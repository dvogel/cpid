#![allow(unused_imports)]
#![allow(unused_variables)]

use std::collections::HashMap;
use std::convert::identity;
use std::fs;
use std::io;
use std::path::Path;

use anyhow::{bail, Result};
use zip::read::ZipArchive;
use zip::result::ZipResult;

extern crate sled;

const class_packages_tree_suffix: &str = "-class_pkgs";
const package_contents_tree_suffix: &str = "-pkg_classes";

fn tree_name(index_name: &str, suffix: &str) -> String {
    return format!("{}{}", index_name, suffix);
}

fn open_class_packages_tree(db: &sled::Db, index_name: &str) -> sled::Tree {
    return db
        .open_tree(tree_name(index_name, class_packages_tree_suffix))
        .expect("database tree");
}

fn open_package_contents_tree(db: &sled::Db, index_name: &str) -> sled::Tree {
    return db
        .open_tree(tree_name(index_name, package_contents_tree_suffix))
        .expect("database tree");
}

pub fn enumerate_indexes(db: &sled::Db, index_name: &str) -> Result<()> {
    let class_packages_tree = open_class_packages_tree(db, index_name);

    println!("IDX: {} (class -> packages)", index_name);
    for next_result in class_packages_tree.iter() {
        match next_result {
            Ok((kbytes, vbytes)) => {
                String::from_utf8(Vec::from(kbytes.as_ref()))
                    .map(|class_name| println!("CLASS: {}", class_name))
                    .map_err(|e| eprintln!("ERROR: {}", e));
            }
            Err(e) => {
                eprintln!("ERROR: {}", e);
            }
        }
    }

    let package_contents_tree = open_package_contents_tree(db, index_name);
    println!("IDX: {} (package -> classes)", index_name);
    for next_result in package_contents_tree.iter() {
        match next_result {
            Ok((kbytes, vbytes)) => {
                String::from_utf8(Vec::from(kbytes.as_ref()))
                    .map(|package_name| println!("PACKAGE: {}", package_name))
                    .map_err(|e| eprintln!("ERROR: {}", e));
            }
            Err(e) => {
                eprintln!("ERROR: {}", e);
            }
        }
    }

    Ok(())
}

pub fn index_class_tuples(
    class_packages_tree: &sled::Tree,
    package_contents_tree: &sled::Tree,
    tuples: &[(String, String, String)],
) -> Result<(), String> {
    for (class_name, package_name, zip_path) in tuples {
        println!("({}, {})", class_name, package_name);
        class_packages_tree
            .update_and_fetch(class_name, |bytes: Option<&[u8]>| {
                merge_string_into_list(package_name, bytes)
            })
            .map_err(|e| e.to_string())?;

        package_contents_tree
            .update_and_fetch(package_name, |bytes: Option<&[u8]>| {
                merge_string_into_list(class_name, bytes)
            })
            .map_err(|e| e.to_string())?;
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
                    0..=1 => eprintln!(
                        "Skipping because it lack enough path components: '{}'",
                        filename
                    ),
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

pub fn merge_string_into_list(new_entry: &str, old_bytes: Option<&[u8]>) -> Option<Vec<u8>> {
    let mut list: Vec<String> = old_bytes
        .and_then(|b| serde_json::from_slice(b).ok())
        .unwrap_or_else(|| Vec::new());

    list.push(new_entry.to_string());
    list.sort();
    list.dedup();

    serde_json::to_string(&list)
        .map(|b| b.into_bytes())
        .map_err(|e| eprintln!("WTF?! {}", e))
        .ok()
}

pub fn query_class_index(
    db: &sled::Db,
    index_name: &str,
    class_name: &str,
) -> Result<HashMap<String, Vec<String>>> {
    let class_packages_tree = open_class_packages_tree(db, index_name);
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
    let class_packages_tree = open_class_packages_tree(db, index_name);
    let package_contents_tree = open_package_contents_tree(db, index_name);
    walk_file_tree(indexed_dir_path, &|entry: &fs::DirEntry| {
        if let Some(entry_path) = entry.path().as_path().to_str() {
            if entry_path.ends_with(".jar") {
                index_zip_archive(entry.path().as_path())
                    .map(|tuples| {
                        index_class_tuples(&class_packages_tree, &package_contents_tree, &tuples)
                    })
                    .map(|_| {
                        class_packages_tree
                            .flush()
                            .and(package_contents_tree.flush())
                    })
                    .map_err(|e| {
                        eprintln!(
                            "Error: Could not store index entries for archive: {}: {}",
                            entry_path, e
                        );
                    });
            }
        }
    })
    .expect("Directory scanning.");
    class_packages_tree.flush()?;
    Ok(())
}

pub fn reindex_classpath(db: &sled::Db, index_name: &str, class_path: &str) -> Result<()> {
    let class_packages_tree = open_class_packages_tree(db, index_name);
    let package_contents_tree = open_package_contents_tree(db, index_name);
    for jar_path_name in class_path.split(':') {
        if jar_path_name.ends_with(".jar") {
            // TODO: This chain needs to be pulled out into a new function to consolidate with
            // reindex_jar_dir
            index_zip_archive(Path::new(jar_path_name))
                .map(|tuples| {
                    index_class_tuples(&class_packages_tree, &package_contents_tree, &tuples)
                })
                .map(|_| {
                    class_packages_tree
                        .flush()
                        .and(package_contents_tree.flush())
                })
                .map_err(|e| {
                    eprintln!(
                        "Error: Could not store index entries for archive: {}: {}",
                        jar_path_name, e
                    );
                });
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
