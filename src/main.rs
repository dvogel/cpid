#![allow(unused_imports)]
#![allow(unused_variables)]

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::path::Path;
use std::thread;
use std::time;
use std::os::unix::net::{UnixStream, UnixListener};

extern crate serde_derive;
extern crate serde_json;
extern crate sled;

use anyhow::{bail, Result};
// use serde::{Deserialize, Serialize};
use serde_derive::{Serialize};
use zip::read::ZipArchive;
use zip::result::ZipResult;

mod proto;
mod indexes;

// types       SomeClassName: [my.pacakge.name, my.package.name.SomeClassName.class, my.package.name.jar]
// type2pkg    SomeClassName: my.package.name
// type2cfile  SomeClassName: my.pacakge.name.SomeClassName.class
// type2jar    SomeClassName: my.package.name.jar

// query: anyImportable([MyClassName, OtherClassName, ThingInPackage])


// {
//     "ClassName": [
//         ["package.name", "filename.ext"],
//         ["other.pacakge.name", "filename.ext"],
//     ]
// }

// #[derive(Debug, PartialEq, Deserialize, Serialize)]
// struct JarClassPackages(Vec<String>);


fn create_or_open_db() -> Result<sled::Db> {
    let xdg = xdg::BaseDirectories::with_prefix("cpid").expect("XDG initialization.");
    let db_path = xdg.place_data_file(Path::new("findex")).expect("writeable XDG data directory.");
    let db = sled::open(db_path).expect("writable database file.");
    Ok(db)
}

fn default_socket_path() -> Result<String> {
    let xdg = xdg::BaseDirectories::with_prefix("cpid").expect("XDG initialization.");
    let db_path = xdg.place_state_file(Path::new("sock")).expect("writeable XDG state directory.");
    let usable_db_path = db_path.as_path().to_str().unwrap(); //ok_or(Err(String::from("usable XDG state directory path.")))?;
    Ok(String::from(usable_db_path))
}

#[derive(PartialEq, Serialize)]
struct ClassQueryResponse {
    results: HashMap<String, Vec<String>>
}

impl ClassQueryResponse {
    fn new() -> Self {
        ClassQueryResponse{
            results: HashMap::new()
        }
    }
}

fn serve(db: &sled::Db, socket_path: String) -> Result<()> {
    let listener = UnixListener::bind(&socket_path)?;
    println!("Listening on {}", socket_path);
    for client in listener.incoming() {
        match client {
            Ok(stream) => {
                let db1 = db.clone();
                thread::spawn(move || {
                    proto::handle_client(db1, stream);
                });
            },
            Err(e) => {
                eprintln!("Failed to handle incoming client: {}", e);
            }
        }
    }
    std::fs::remove_file(socket_path)?;
    Ok(())
}

fn main() -> Result<()> {
    const USAGE_TEXT: &str = r#"
        USAGE: cpid <reindex|enumerate|serve> ...

        cpid clsquery <index_name> <class_name>
        cpid reindex <index_name> <srcdir>
        cpid reindex <index_name> <classpath_expr>
        cpid enumerate [pattern]
        cpid serve [/socket/path]
        "#;

    let subcmd = std::env::args().nth(1).expect(USAGE_TEXT);
    let db = create_or_open_db().expect("writable database file.");
    let subcmd_result = match subcmd.as_str() {
        "clsquery" => {
            let index_name = std::env::args().nth(2).expect(USAGE_TEXT);
            let class_name = std::env::args().nth(3).expect(USAGE_TEXT);
            let results = indexes::query_class_index(&db, &index_name, &class_name)?;
            let resp = ClassQueryResponse{
                results: results
            };
            println!("{}", serde_json::to_string(&resp)?);
            Ok(())
        },
        "enumerate" => {
            let index_name = std::env::args().nth(2).expect(USAGE_TEXT);
            indexes::enumerate_indexes(&db, &index_name)
        },
        "reindex" => {
            let index_name = std::env::args().nth(2).expect(USAGE_TEXT);
            let jar_source = std::env::args().nth(3).expect(USAGE_TEXT);
            let jar_source_path = Path::new(&jar_source);
            if jar_source_path.is_dir() {
                indexes::reindex_jar_dir(&db, &index_name, jar_source_path)
            } else {
                indexes::reindex_classpath(&db, &index_name, &jar_source)
            }
        },
        "serve" => {
            let default_path = default_socket_path()?;
            let path = std::env::args().nth(2).or(Some(default_path)).unwrap();
            serve(&db, path)
        },
        _ => { bail!(&USAGE_TEXT); }
    };
    drop(db);
    subcmd_result
}
