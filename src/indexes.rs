#![allow(unused_imports)]
#![allow(unused_variables)]

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use anyhow::{bail, Result};
use zip::read::ZipArchive;
use zip::result::ZipResult;

extern crate sled;

pub fn enumerate_indexes(db: &sled::Db, index_name: &str) -> Result<()> {
    let class_packages_tree = db.open_tree(index_name).expect("database tree");

    for next_result in class_packages_tree.iter() {
        match next_result {
            Ok((kbytes, vbytes)) => {
                match std::str::from_utf8(&kbytes) {
                    Ok(kstr) => { println!("CLASS: {}", kstr); },
                    Err(e) => { eprintln!("ERROR: {}", e); }
                }
            },
            Err(e) => { eprintln!("ERROR: {}", e); }
        }
    }

    Ok(())
}

pub fn index_class_tuples(tree: &sled::Tree, tuples: &[(String, String, String)]) -> Result<(), String> {
    for (class_name, package_name, zip_path) in tuples {
        tree.update_and_fetch(class_name, |bytes: Option<&[u8]>| {
            merge_package_into_list(package_name, bytes)
        }).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn index_zip_archive(path: &Path) -> Result<Vec<(String, String, String)>> {
    let inf = fs::File::open(path)?;
    let archive = ZipArchive::new(inf)?;

    let mut accum: Vec<(String, String, String)> = Vec::new();
    for filename in archive.file_names() {
        if filename.ends_with(".class") && !filename.contains('$') {
            if let Some(stub) = filename.strip_suffix(".class") {
                let mut parts = stub.split('/').collect::<Vec<&str>>();
                match parts.len() {
                    0..=1 => eprintln!("Skipping because it lack enough path components: '{}'", filename),
                    _ => {
                        if let Some(class_name) = parts.pop() {
                            let package_name = parts.join(".");
                            accum.push((
                                    class_name.to_string(),
                                    package_name.to_string(),
                                    filename.to_string(),
                                    ));
                        }
                    }
                }
            }
        }
    }
    Ok(accum)
}

pub fn merge_package_into_list(package_name: &str, old_bytes: Option<&[u8]>) -> Option<Vec<u8>> {
    let mut packages: Vec<String> = match old_bytes {
        None => Vec::new(),
        Some(b) => {
            match serde_json::from_slice(b) {
                Err(_) => Vec::new(),
                Ok(entries) => entries,
            }
        }
    };
    packages.push(package_name.to_string());
    packages.sort();
    packages.dedup();
    match serde_json::to_string(&packages) {
        Ok(b) => Some(b.into_bytes()),
        Err(_) => {
            eprintln!("WTF?");
            None
        }
    }
}

pub fn query_class_index(db: &sled::Db, index_name: &str, class_name: &str) -> Result<HashMap<String, Vec<String>>> {
    let class_packages_tree = db.open_tree(index_name).expect("database tree");
    let maybe_val = class_packages_tree.get(class_name)?;
    let mut results: HashMap<String, Vec<String>> = HashMap::new();
    match maybe_val {
        None => bail!("class name is unknown: {}", class_name),
        Some(val_bytes) => {
            results.insert(class_name.to_string(), serde_json::from_slice(&val_bytes)?);
        }
    };
    Ok(results)
}

pub fn reindex_jar_dir(db: &sled::Db, index_name: &str, indexed_dir_path: &Path) -> Result<()> {
    let class_packages_tree = db.open_tree(index_name).expect("database tree");
    walk_file_tree(indexed_dir_path, &|entry: &fs::DirEntry| {
        if let Some(entry_path) = entry.path().as_path().to_str() {
            if entry_path.ends_with(".jar") {
                match index_zip_archive(entry.path().as_path()).map_err(|e| e.to_string()) {
                    Ok(tuples) => match index_class_tuples(&class_packages_tree, &tuples) {
                        Ok(()) => {
                            class_packages_tree.flush().unwrap();
                        },
                        Err(e) => {
                            eprintln!("Error: Could not store index entries for archive: {}: {}", entry_path, e);
                        }
                    },
                    Err(e) => {
                        eprintln!("Error: Could not read archive: {}: {}", entry_path, e);
                    }
                }
            }
        }
    }).expect("Directory scanning.");
    class_packages_tree.flush()?;
    Ok(())
}

pub fn reindex_classpath(db: &sled::Db, index_name: &str, class_path: &str) -> Result<()> {
    let class_packages_tree = db.open_tree(index_name).expect("database tree");
    for jar_path_name in class_path.split(':') {
        if jar_path_name.ends_with(".jar")  {
            match index_zip_archive(Path::new(jar_path_name)) {
                Ok(tuples) => match index_class_tuples(&class_packages_tree, &tuples) {
                    Ok(()) => { class_packages_tree.flush().unwrap(); },
                    Err(e) => eprintln!("Error: Could not store index entries for archive: {}: {}", jar_path_name, e),
                },
                Err(e) => eprintln!("Error: Could not store index entries for archive: {}: {}", jar_path_name, e),
            }
        }
    }
    class_packages_tree.flush().unwrap();
    Ok(())
}

pub fn walk_file_tree(dir: &Path, cb: &dyn Fn(&fs::DirEntry)) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry_path.is_dir() {
            walk_file_tree(&entry_path, cb)?;
        } else {
            cb(&entry);
        }
    }
    Ok(())
}

