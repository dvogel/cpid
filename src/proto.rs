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

extern crate serde_derive;
extern crate serde_json;
extern crate sled;

use crate::indexes;

#[derive(Debug, PartialEq, serde_derive::Deserialize)]
pub struct ClassQueryArgs {
    index_name: String,
    class_name: String,
}

#[derive(Debug, PartialEq, serde_derive::Deserialize)]
pub struct ReindexArgs {
    index_name: String,
    archive_source: String,
}

#[derive(Debug, PartialEq, serde_derive::Deserialize)]
pub struct PackageEnumerateArgs {
    pacakge_name: String,
}

#[derive(Debug, PartialEq, serde_derive::Deserialize)]
#[serde(tag = "type")]
pub enum ClientMsg {
    ClassQuery(ClassQueryArgs),
    PackageEnumerateQuery(PackageEnumerateArgs),
    ReindexPathCmd(ReindexArgs),
    ReindexClasspathCmd(ReindexArgs),
    ShutdownCmd,
}

fn fmt_exec_result<T, E: Display>(res: Result<T, E>) -> String {
    match res {
        Ok(_) => "OK".to_string(),
        Err(e) => format!("ERR: {}", e.to_string()),
    }
}

fn exec_class_query(db: &sled::Db, msg: ClassQueryArgs) -> Result<String> {
    let results = indexes::query_class_index(&db, &msg.index_name, &msg.class_name)?;
    let encoded = serde_json::to_string::<HashMap<String, Vec<String>>>(&results)?;
    Ok(encoded)
}

fn exec_reindex_classpath_cmd(db: &sled::Db, msg: ReindexArgs) -> Result<String> {
    indexes::reindex_classpath(&db, &msg.index_name, &msg.archive_source)?;
    let encoded = "{}".to_string();
    Ok(encoded)
}

fn exec_reindex_path_cmd(db: &sled::Db, msg: ReindexArgs) -> Result<String> {
    let path = Path::new(&msg.archive_source);
    indexes::reindex_jar_dir(&db, &msg.index_name, path)?;
    let encoded = "{}".to_string();
    Ok(encoded)
}

pub fn handle_client<I: Read, O: Write>(
    db: sled::Db,
    mut instream: I,
    mut outstream: O,
    mut shutdown_cond: Arc<AtomicBool>,
) -> () {
    let msgs = serde_json::Deserializer::from_reader(io::BufReader::new(instream));
    for msg in msgs.into_iter::<ClientMsg>() {
        match msg {
            Ok(msg1) => {
                let encoded_resp = match msg1 {
                    ClientMsg::ClassQuery(args) => exec_class_query(&db, args),
                    ClientMsg::PackageEnumerateQuery(args) => {
                        Err(Error::msg("Not implemented yet."))
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
                match encoded_resp {
                    Ok(s) => {
                        outstream.write(s.as_bytes());
                        outstream.flush();
                    }
                    Err(e) => {
                        eprintln!("ERR: {}", e);
                        break;
                    }
                };
            }
            Err(e) => {
                eprintln!("ERR: {}", e);
                break;
            }
        }
    }

    outstream.flush();
}
