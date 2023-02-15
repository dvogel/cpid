#![allow(unused_imports)]
#![allow(unused_variables)]

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{stdin, stdout, Read};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time;

extern crate serde_derive;
extern crate serde_json;
extern crate sled;

use anyhow::{anyhow, bail, Context, Error, Result};
use regex::Regex;
// use serde::{Deserialize, Serialize};
use serde_derive::Serialize;
use zip::read::ZipArchive;
use zip::result::ZipResult;

use cpid;
use cpid::indexes::Index;
use cpid::jdk::is_jimage_file;
use cpid::project::crawl_project;

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
    let xdg = xdg::BaseDirectories::with_prefix("cpid")
        .map_err(|e| Error::msg("XDG library initialization failed."))?;
    let db_path = xdg
        .place_data_file(Path::new("findex"))
        .with_context(|| Error::msg("The XDG data directory is not writable."))?;
    let db = sled::open(db_path)?;
    Ok(db)
}

fn default_socket_path() -> Result<String> {
    let xdg = xdg::BaseDirectories::with_prefix("cpid")
        .map_err(|e| Error::msg("XDG library initialization fails."))?;
    let db_path = xdg
        .place_state_file(Path::new("sock"))
        .with_context(|| Error::msg("The XDG state directory is not writable."))?;
    let usable_db_path = db_path
        .as_path()
        .to_str()
        .ok_or_else(|| Error::msg("The XDG state directory path is not UNICODE-compatible."))?;
    Ok(String::from(usable_db_path))
}

fn usage_error(trailer: &str) -> anyhow::Error {
    const USAGE_TEXT: &str = r#"
        USAGE: cpid <reindex|enumerate|serve> ...

        cpid clsquery <index_name> <class_name>
        cpid pkgenum <index_name> <package_name>
        cpid reindex classpath <classpath_expr>
        cpid reindex jardir <index_name> <jardir>
        cpid reindex jimage <index_name> <image_path>
        cpid reindex project <index_name> <srcdir>
        cpid indexes
        cpid enumerate <index_name> [pattern]
        cpid serve [/socket/path]
        "#;

    anyhow!("{}\n{}", &USAGE_TEXT, trailer)
}

fn reindex_classpath_main(db: &sled::Db) -> Result<()> {
    let index_name = std::env::args()
        .nth(3)
        .ok_or_else(|| usage_error("Index name required."))?;
    let classpath = std::env::args()
        .nth(4)
        .ok_or_else(|| usage_error("Classpath required."))?;
    cpid::indexes::reindex_classpath(&Index::new(&db, &index_name), &classpath)
}

fn reindex_jardir_main(db: &sled::Db) -> Result<()> {
    let index_name = std::env::args()
        .nth(3)
        .ok_or_else(|| usage_error("Index name required."))?;
    let jar_source = std::env::args()
        .nth(4)
        .ok_or_else(|| usage_error("Jar dir required."))?;
    let jar_source_path = Path::new(&jar_source);
    if jar_source_path.is_dir() {
        cpid::indexes::reindex_jar_dir(&Index::new(&db, &index_name), jar_source_path)
    } else {
        Err(anyhow!("{jar_source} is not a directory."))
    }
}

fn reindex_jimage_main(db: &sled::Db) -> Result<()> {
    let index_name = std::env::args()
        .nth(3)
        .ok_or_else(|| usage_error("Index name required."))?;
    let jar_source = std::env::args()
        .nth(4)
        .ok_or_else(|| usage_error("jimage file path required."))?;
    let jar_source_path = Path::new(&jar_source);
    if jar_source_path.is_file() && is_jimage_file(&jar_source) {
        cpid::indexes::reindex_jimage(&Index::new(&db, &index_name), jar_source_path)
    } else {
        Err(anyhow!("{jar_source} is not a jimage file."))
    }
}

fn reindex_project_main(db: &sled::Db) -> Result<()> {
    let index_name = std::env::args()
        .nth(3)
        .ok_or_else(|| usage_error("Index name required."))?;
    let path_arg = std::env::args()
        .nth(4)
        .ok_or_else(|| usage_error("Project path required."))?;
    let proj_path = Path::new(&path_arg);
    if proj_path.is_dir() {
        cpid::indexes::reindex_project_path(&Index::new(&db, &index_name), &proj_path)
    } else {
        Err(Error::msg("Project path must be a directory."))
    }
}

fn reindex_main(db: &sled::Db) -> Result<()> {
    let subcmd = std::env::args()
        .nth(2)
        .ok_or_else(|| usage_error("Incomplete command line"))?;
    match subcmd.as_str() {
        "classpath" => reindex_classpath_main(db),
        "jardir" => reindex_jardir_main(db),
        "jimage" => reindex_jimage_main(db),
        "project" => reindex_project_main(db),
        _ => Err(usage_error("Unknown reindex sub-command.")),
    }
}

fn main() -> Result<()> {
    let subcmd = std::env::args()
        .nth(1)
        .ok_or_else(|| usage_error("Incomplete command line."))?;
    let db = create_or_open_db()?;
    let subcmd_result = match subcmd.as_str() {
        "clsquery" => {
            let index_name = std::env::args()
                .nth(2)
                .ok_or_else(|| usage_error("Index name required."))?;
            let class_name = std::env::args()
                .nth(3)
                .ok_or_else(|| usage_error("Class name required."))?;
            let results = Index::new(&db, &index_name).query_class_index(&class_name)?;
            println!("{}", serde_json::to_string(&results)?);
            Ok(())
        }
        "pkgenum" => {
            let index_name = std::env::args()
                .nth(2)
                .ok_or_else(|| usage_error("Index name required."))?;
            let pkg_name = std::env::args()
                .nth(3)
                .ok_or_else(|| usage_error("Package name required."))?;
            let results = Index::new(&db, &index_name).query_package_index(&pkg_name)?;
            println!("{}", serde_json::to_string(&results)?);
            Ok(())
        }
        "dropindex" => {
            let index_name = std::env::args()
                .nth(2)
                .ok_or_else(|| usage_error("Index name required."))?;
            Index::new(&db, &index_name).drop_trees()
        }
        "enumerate" => {
            let index_name = std::env::args()
                .nth(2)
                .ok_or_else(|| usage_error("Index name required."))?;
            cpid::indexes::enumerate_indexes(&Index::new(&db, &index_name))
        }
        "indexes" => {
            let index_name_pat = Regex::new(r"(.+)-class_pkgs").unwrap();
            for name_bytes in db.tree_names() {
                let name = String::from_utf8_lossy(name_bytes.as_ref());
                if let Some(caps) = index_name_pat.captures(&name) {
                    println!("{}", caps.get(1).unwrap().as_str());
                }
            }
            Ok(())
        }
        "reindex" => reindex_main(&db),
        "serve" => {
            let default_path = default_socket_path()?;
            let path = std::env::args().nth(2).or(Some(default_path)).unwrap();
            if path == "-" {
                cpid::serve::serve_stdio(&db, stdin(), stdout())
            } else {
                cpid::serve::serve_unix(&db, path)
            }
        }
        _ => Err(usage_error("Unknown sub-command")),
    };
    drop(db);
    subcmd_result
}
