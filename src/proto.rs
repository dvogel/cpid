#![allow(unused_imports)]
#![allow(unused_variables)]

use std::collections::HashMap;
use std::fmt::Display;
use std::io;
use std::io::Write;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;

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
}

fn fmt_exec_result<T, E: Display>(res: Result<T, E>) -> String {
    match res {
        Ok(_) => "OK".to_string(),
        Err(e) => format!("ERR: {}", e.to_string()),
    }
}

fn exec_class_query(db: &sled::Db, msg: ClassQueryArgs) -> Result<String> {
    eprintln!(
        "ClassQuery(index_name: {}, class_name: {})",
        msg.index_name, msg.class_name
    );
    let results = indexes::query_class_index(&db, &msg.index_name, &msg.class_name)?;
    let encoded = serde_json::to_string::<HashMap<String, Vec<String>>>(&results)?;
    Ok(encoded)
}

fn exec_reindex_classpath_cmd(db: &sled::Db, msg: ReindexArgs) -> Result<String> {
    eprintln!(
        "ReindexClasspathCmd(index_name: {}, archive_source: {})",
        msg.index_name, msg.archive_source
    );
    indexes::reindex_classpath(&db, &msg.index_name, &msg.archive_source)?;
    let encoded = "{}".to_string();
    Ok(encoded)
}

fn exec_reindex_path_cmd(db: &sled::Db, msg: ReindexArgs) -> Result<String> {
    eprintln!(
        "ReindexPathCmd(index_name: {}, archive_source: {})",
        msg.index_name, msg.archive_source
    );
    let path = Path::new(&msg.archive_source);
    indexes::reindex_jar_dir(&db, &msg.index_name, path)?;
    let encoded = "{}".to_string();
    Ok(encoded)
}

pub fn handle_client(db: sled::Db, mut stream: UnixStream) -> () {
    let mut write_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            stream.flush();
            stream.shutdown(Shutdown::Both);
            return;
        }
    };

    let msgs = serde_json::Deserializer::from_reader(io::BufReader::new(&stream));
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
                };
                match encoded_resp {
                    Ok(s) => {
                        write_stream.write(s.as_bytes());
                    }
                    Err(e) => eprintln!("ERR: {}", e),
                };
            }
            Err(e) => {
                break;
            }
        }
    }

    stream.flush();
    stream.shutdown(Shutdown::Both);
}
