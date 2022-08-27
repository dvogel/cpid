#![allow(unused_imports)]
#![allow(unused_variables)]

use std::collections::HashMap;
use std::fmt::Display;
use std::io;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use anyhow::{bail, Error, Result};
use serde_derive::Serialize;

extern crate serde_derive;
extern crate serde_json;
extern crate sled;

use crate::indexes;
use crate::jdk::is_jimage_file;

#[derive(Debug, PartialEq, serde_derive::Deserialize)]
pub struct ClassQueryArgs {
    index_name: String,
    class_name: String,
}

#[derive(Debug, PartialEq, serde_derive::Deserialize)]
pub struct ClassMultiQueryArgs {
    index_names: Vec<String>,
    class_name: String,
}

#[derive(Debug, PartialEq, serde_derive::Deserialize)]
pub struct ReindexArgs {
    index_name: String,
    archive_source: String,
}

#[derive(Debug, PartialEq, serde_derive::Deserialize)]
pub struct PackageEnumerateArgs {
    index_name: String,
    package_name: String,
}

#[derive(Debug, PartialEq, serde_derive::Deserialize)]
pub struct PackageMultiEnumerateArgs {
    index_names: Vec<String>,
    package_name: String,
}

#[derive(Debug, PartialEq, serde_derive::Deserialize)]
#[serde(tag = "type")]
pub enum ClientMsg {
    ClassQuery(ClassQueryArgs),
    ClassMultiQuery(ClassMultiQueryArgs),
    PackageEnumerateQuery(PackageEnumerateArgs),
    PackageMultiEnumerateQuery(PackageMultiEnumerateArgs),
    ReindexPathCmd(ReindexArgs),
    ReindexClasspathCmd(ReindexArgs),
    ShutdownCmd,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct ClassQueryResponseArgs {
    pub results: HashMap<String, Vec<String>>,
}

impl ClassQueryResponseArgs {
    pub fn new(results: HashMap<String, Vec<String>>) -> Self {
        Self { results }
    }
}

#[derive(Debug, PartialEq, Serialize)]
pub struct PackageEnumerateQueryResponseArgs {
    pub results: HashMap<String, Vec<String>>,
}

impl PackageEnumerateQueryResponseArgs {
    pub fn new(results: HashMap<String, Vec<String>>) -> Self {
        Self { results }
    }
}

#[derive(Debug, PartialEq, serde_derive::Serialize)]
#[serde(tag = "type")]
pub enum ResponseMsg {
    ClassQueryResponse(ClassQueryResponseArgs),
    PackageEnumerateQueryResponse(PackageEnumerateQueryResponseArgs),
    NullResponse,
}

#[derive(Debug, PartialEq, serde_derive::Deserialize)]
pub struct ChannelMsg(u32, ClientMsg);

#[derive(Debug, PartialEq, serde_derive::Serialize)]
pub struct ChannelResponse(u32, ResponseMsg);

fn fmt_exec_result<T, E: Display>(res: Result<T, E>) -> String {
    match res {
        Ok(_) => "OK".to_string(),
        Err(e) => format!("ERR: {}", e.to_string()),
    }
}

fn exec_class_query(db: &sled::Db, msg: ClassQueryArgs) -> Result<ResponseMsg> {
    let results = indexes::query_class_index(&db, &msg.index_name, &msg.class_name)?;
    Ok(ResponseMsg::ClassQueryResponse(
        ClassQueryResponseArgs::new(results),
    ))
}

fn exec_class_multi_query(db: &sled::Db, msg: ClassMultiQueryArgs) -> Result<ResponseMsg> {
    let mut results: HashMap<String, Vec<String>> = HashMap::new();
    for idx_name in msg.index_names {
        let results1 = indexes::query_class_index(&db, &idx_name, &msg.class_name)?;
        results.extend(results1);
    }

    Ok(ResponseMsg::ClassQueryResponse(
        ClassQueryResponseArgs::new(results),
    ))
}

fn exec_package_enumerate_query(db: &sled::Db, msg: PackageEnumerateArgs) -> Result<ResponseMsg> {
    let results = indexes::query_package_index(&db, &msg.index_name, &msg.package_name)?;
    Ok(ResponseMsg::PackageEnumerateQueryResponse(
        PackageEnumerateQueryResponseArgs::new(results),
    ))
}

fn exec_package_multi_enumerate_query(
    db: &sled::Db,
    msg: PackageMultiEnumerateArgs,
) -> Result<ResponseMsg> {
    let mut results: HashMap<String, Vec<String>> = HashMap::new();
    for idx_name in msg.index_names {
        let results1 = indexes::query_package_index(&db, &idx_name, &msg.package_name)?;
        results.extend(results1);
    }

    Ok(ResponseMsg::PackageEnumerateQueryResponse(
        PackageEnumerateQueryResponseArgs::new(results),
    ))
}

fn exec_reindex_classpath_cmd(db: &sled::Db, msg: ReindexArgs) -> Result<ResponseMsg> {
    indexes::reindex_classpath(&db, &msg.index_name, &msg.archive_source)?;
    Ok(ResponseMsg::NullResponse)
}

fn exec_reindex_path_cmd(db: &sled::Db, msg: ReindexArgs) -> Result<ResponseMsg> {
    let path = Path::new(&msg.archive_source);
    let pathStr = path.to_str().ok_or_else(|| {
        anyhow::Error::msg("Invalid archive source path provided in protocol message.")
    })?;
    if path.is_file() && is_jimage_file(pathStr) {
        indexes::reindex_jimage(&db, &msg.index_name, path)?;
    } else if path.is_dir() {
        indexes::reindex_jar_dir(&db, &msg.index_name, path)?;
    }
    Ok(ResponseMsg::NullResponse)
}

pub fn handle_client<I: Read, O: Write>(
    db: sled::Db,
    instream: I,
    mut outstream: O,
    shutdown_cond: Arc<AtomicBool>,
) -> () {
    let msgs = serde_json::Deserializer::from_reader(io::BufReader::new(instream));
    for msg in msgs.into_iter::<ChannelMsg>() {
        match msg {
            Ok(msg1) => {
                let resp_msg: Result<ResponseMsg> = match msg1.1 {
                    ClientMsg::ClassQuery(args) => exec_class_query(&db, args),
                    ClientMsg::ClassMultiQuery(args) => exec_class_multi_query(&db, args),
                    ClientMsg::PackageEnumerateQuery(args) => {
                        exec_package_enumerate_query(&db, args)
                    }
                    ClientMsg::PackageMultiEnumerateQuery(args) => {
                        exec_package_multi_enumerate_query(&db, args)
                    }
                    ClientMsg::ReindexClasspathCmd(args) => exec_reindex_classpath_cmd(&db, args),
                    ClientMsg::ReindexPathCmd(args) => exec_reindex_path_cmd(&db, args),
                    ClientMsg::ShutdownCmd => {
                        shutdown_cond.store(true, Ordering::SeqCst);
                        eprintln!("Client requested shutdown.");
                        break;
                    }
                    _ => Err(Error::msg("Unrecognized message type.")),
                };
                let write_result = resp_msg
                    .and_then(|m| {
                        serde_json::to_string::<ChannelResponse>(&ChannelResponse(msg1.0, m))
                            .map_err(|e| anyhow::Error::new(e))
                    })
                    .and_then(|s| {
                        outstream
                            .write_all(s.as_bytes())
                            .and_then(|_| outstream.flush())
                            .map_err(|e| anyhow::Error::new(e))
                    })
                    .map_err(|e| eprintln!("ERR: {}", e));
                if let Err(e) = write_result {
                    break;
                }
            }
            Err(e) => {
                eprintln!("ERR: {}", e);
                break;
            }
        }
    }

    outstream.flush();
}
