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
use clap::Parser;
use regex::Regex;
// use serde::{Deserialize, Serialize};
use serde_derive::Serialize;
use zip::read::ZipArchive;
use zip::result::ZipResult;

use cpid::cli;
use cpid::indexes::{
    enumerate_indexes, reindex_classpath, reindex_jar_dir, reindex_jimage, reindex_project_path,
    Index,
};
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

fn main() -> Result<()> {
    let args = cli::CmdLineArgs::parse();
    let db = create_or_open_db()?;

    let subcmd_result = match args.command {
        cli::Commands::ClsQuery {
            index_name,
            class_name,
        } => {
            let results = Index::new(&db, &index_name).query_class_index(&class_name)?;
            println!("{}", serde_json::to_string(&results)?);
            Ok(())
        }
        cli::Commands::PkgEnum {
            index_name,
            package_name,
        } => {
            let results = Index::new(&db, &index_name).query_package_index(&package_name)?;
            println!("{}", serde_json::to_string(&results)?);
            Ok(())
        }
        cli::Commands::DropIndex { index_name } => Index::new(&db, &index_name).drop_trees(),
        cli::Commands::Reindex { reindex_command } => match reindex_command {
            cli::ReindexCommands::Classpath {
                index_name,
                classpath_expr,
            } => reindex_classpath(&Index::new(&db, &index_name), &classpath_expr),
            cli::ReindexCommands::JarDir {
                index_name,
                jar_dir,
            } => {
                let jar_source_path = Path::new(&jar_dir);
                if jar_source_path.is_dir() {
                    reindex_jar_dir(&Index::new(&db, &index_name), jar_source_path)
                } else {
                    Err(anyhow!("{jar_dir} is not a directory."))
                }
            }
            cli::ReindexCommands::JImage {
                index_name,
                image_file,
            } => {
                let image_path = Path::new(&image_file);
                if image_path.is_file() && is_jimage_file(&image_file) {
                    reindex_jimage(&Index::new(&db, &index_name), image_path)
                } else {
                    Err(anyhow!("{image_file} is not a jimage file."))
                }
            }
            cli::ReindexCommands::Project {
                index_name,
                src_dir,
            } => {
                let proj_path = Path::new(&src_dir);
                if proj_path.is_dir() {
                    reindex_project_path(&Index::new(&db, &index_name), proj_path)
                } else {
                    Err(Error::msg("Project path must be a directory."))
                }
            }
        },
        cli::Commands::Indexes => {
            let index_name_pat = Regex::new(r"(.+)-class_pkgs").unwrap();
            for name_bytes in db.tree_names() {
                let name = String::from_utf8_lossy(name_bytes.as_ref());
                if let Some(caps) = index_name_pat.captures(&name) {
                    println!("{}", caps.get(1).unwrap().as_str());
                }
            }
            Ok(())
        }
        cli::Commands::Enumerate { index_name } => enumerate_indexes(&Index::new(&db, &index_name)),
        cli::Commands::Serve { socket_path } => {
            let default_path = default_socket_path()?;
            let path = socket_path.unwrap_or(default_path);
            if path == "-" {
                cpid::serve::serve_stdio(&db, stdin(), stdout())
            } else {
                cpid::serve::serve_unix(&db, path)
            }
        }
    };

    drop(db);
    subcmd_result
}
