#![allow(unused_imports)]
#![allow(unused_variables)]

use std::collections::HashMap;
use std::convert::identity;
use std::fs;
use std::io;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, Result};
use regex::Regex;
use zip::read::ZipArchive;
use zip::result::ZipResult;

extern crate sled;

const CLASS_PACKAGES_TREE_SUFFIX: &str = "-class_pkgs";
const PACKAGE_CONTENTS_TREE_SUFFIX: &str = "-pkg_classes";

fn tree_name(index_name: &str, suffix: &str) -> String {
    return format!("{}{}", index_name, suffix);
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

pub struct Index<'a> {
    db: &'a sled::Db,
    index_name: &'a str,
}

impl<'a> Index<'a> {
    pub fn new(db: &'a sled::Db, index_name: &'a str) -> Self {
        Index { db, index_name }
    }

    pub fn name(&self) -> &'a str {
        self.index_name
    }

    pub fn open_class_packages_tree(&self) -> sled::Tree {
        return self
            .db
            .open_tree(tree_name(self.index_name, CLASS_PACKAGES_TREE_SUFFIX))
            .expect("database tree");
    }

    pub fn open_package_contents_tree(&self) -> sled::Tree {
        return self
            .db
            .open_tree(tree_name(self.index_name, PACKAGE_CONTENTS_TREE_SUFFIX))
            .expect("database tree");
    }

    pub fn drop_trees(&self) -> Result<()> {
        self.db
            .drop_tree(tree_name(self.index_name, CLASS_PACKAGES_TREE_SUFFIX))?;
        self.db
            .drop_tree(tree_name(self.index_name, PACKAGE_CONTENTS_TREE_SUFFIX))?;
        Ok(())
    }

    pub fn index_class_tuples(&self, tuples: &[(String, String, String)]) -> Result<()> {
        let class_packages_tree = self.open_class_packages_tree();
        let package_contents_tree = self.open_package_contents_tree();

        for (class_name, package_name, zip_path) in tuples {
            class_packages_tree
                .update_and_fetch(class_name, |bytes: Option<&[u8]>| {
                    merge_string_into_list(package_name, bytes)
                })
                .map_err(|e| anyhow!(e.to_string()))?;

            package_contents_tree
                .update_and_fetch(package_name, |bytes: Option<&[u8]>| {
                    merge_string_into_list(class_name, bytes)
                })
                .map_err(|e| anyhow!(e.to_string()))?;
        }

        class_packages_tree.flush()?;
        package_contents_tree.flush()?;
        Ok(())
    }

    pub fn query_class_index(&self, class_name: &str) -> Result<HashMap<String, Vec<String>>> {
        let maybe_val = self.open_class_packages_tree().get(class_name)?;
        let mut results: HashMap<String, Vec<String>> = HashMap::new();
        match maybe_val {
            None => {
                results.insert(class_name.to_string(), Vec::new());
            }
            Some(val_bytes) => {
                results.insert(class_name.to_string(), serde_json::from_slice(&val_bytes)?);
            }
        };
        Ok(results)
    }

    pub fn query_package_index(&self, package_name: &str) -> Result<HashMap<String, Vec<String>>> {
        let maybe_val = self.open_package_contents_tree().get(package_name)?;
        let mut results: HashMap<String, Vec<String>> = HashMap::new();
        match maybe_val {
            None => {
                results.insert(package_name.to_string(), Vec::new());
            }
            Some(val_bytes) => {
                results.insert(
                    package_name.to_string(),
                    serde_json::from_slice(&val_bytes)?,
                );
            }
        };
        Ok(results)
    }
}

// TODO: This should be split up. Half of this should be in `impl Index`, returning an iterator.
// The iterator should be consumed and output generated in main.rs.
pub fn enumerate_indexes(index: &Index) -> Result<()> {
    println!("IDX: {} (class -> packages)", index.name());
    for next_result in index.open_class_packages_tree().iter() {
        match next_result {
            Ok((kbytes, vbytes)) => {
                let _ = String::from_utf8(Vec::from(kbytes.as_ref()))
                    .map(|class_name| println!("CLASS: {}", class_name))
                    .map_err(|e| eprintln!("ERROR: {}", e));
            }
            Err(e) => {
                eprintln!("ERROR: {}", e);
            }
        }
    }

    println!("IDX: {} (package -> classes)", index.name());
    for next_result in index.open_package_contents_tree().iter() {
        match next_result {
            Ok((kbytes, vbytes)) => {
                let _ = String::from_utf8(Vec::from(kbytes.as_ref()))
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

pub fn index_jimage(path: &Path) -> Result<Vec<(String, String, String)>> {
    let jimage_child = Command::new("jimage")
        .arg("list")
        .arg(path)
        .stdout(Stdio::piped())
        .spawn()?;

    let mut accum: Vec<(String, String, String)> = Vec::new();
    let instream = jimage_child
        .stdout
        .map(|r| BufReader::new(r))
        .ok_or(anyhow!("Failed to read jimage process output."))?;

    let module_header_pat = Regex::new(r"^Module: (.+)$").unwrap();
    // This intentionally omits the '$' character used to indicate inner classes.
    let class_entry_pat = Regex::new(r"^\s+([a-z0-9]+[/])+([A-Za-z0-9_]+).class").unwrap();

    for ln_res in instream.lines() {
        if let Ok(ln) = ln_res {
            if let Some(caps) = module_header_pat.captures(&ln) {
                // Not sure what to do with this, if anything.
                let _curr_module = String::from(caps.get(1).unwrap().as_str());
            } else if let Some(caps) = class_entry_pat.captures(&ln) {
                let captured_fname = caps.get(0).unwrap().as_str();
                let filename = String::from(captured_fname.trim());
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
    }

    Ok(accum)
}

pub fn reindex_project_path(index: &Index, indexed_project_path: &Path) -> Result<()> {
    let packages = crate::project::crawl_project(indexed_project_path).or(Err(anyhow!(
        "Failed to crawl contents of project directory."
    )))?;
    let mut tuples: Vec<(String, String, String)> = Vec::new();
    for pkg in packages {
        if let Some(pkg_name) = pkg.name {
            for class_name in pkg.contained_identifiers {
                tuples.push((class_name.clone(), pkg_name.clone(), String::new()));
            }
        }
    }
    index.index_class_tuples(&tuples).or(Err(anyhow!(
        "Failed to index contents of project directory."
    )))?;

    Ok(())
}

pub fn reindex_jar_dir(index: &Index, indexed_dir_path: &Path) -> Result<()> {
    walk_file_tree(indexed_dir_path, &|entry: &fs::DirEntry| {
        if let Some(entry_path) = entry.path().as_path().to_str() {
            if entry_path.ends_with(".jar") {
                index_zip_archive(entry.path().as_path())
                    .and_then(|tuples| index.index_class_tuples(&tuples))
                    .map_err(|e| {
                        anyhow!(
                            "Error: Could not store index entries for archive: {}: {}",
                            entry_path,
                            e
                        )
                    })?;
            }
        }
        Ok(())
    })
    .expect("Directory scanning.");
    Ok(())
}

pub fn reindex_classpath(index: &Index, class_path: &str) -> Result<()> {
    for jar_path_name in class_path.split(':') {
        if jar_path_name.ends_with(".jar") {
            // TODO: This chain needs to be pulled out into a new function to consolidate with
            // reindex_jar_dir
            index_zip_archive(Path::new(jar_path_name))
                .and_then(|tuples| index.index_class_tuples(&tuples))
                .map_err(|e| {
                    anyhow!(
                        "Error: Could not store index entries for archive: {}: {}",
                        jar_path_name,
                        e
                    )
                })?;
        }
    }
    Ok(())
}

pub fn reindex_jimage(index: &Index, jimage_path: &Path) -> Result<()> {
    index_jimage(jimage_path)
        .and_then(|tuples| index.index_class_tuples(&tuples))
        .map_err(|e| {
            anyhow!(
                "Error: Could not store index entries for image: {}: {}",
                jimage_path.to_str().unwrap(),
                e
            )
        })?;
    Ok(())
}

pub fn walk_file_tree(dir: &Path, cb: &dyn Fn(&fs::DirEntry) -> Result<()>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry_path.is_dir() {
            walk_file_tree(&entry_path, cb)?;
        } else {
            cb(&entry)?;
        }
    }
    Ok(())
}
