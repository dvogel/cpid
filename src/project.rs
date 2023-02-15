#![allow(unused_imports)]
#![allow(unused_variables)]

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use anyhow::{Error, Result};
use tree_sitter::{Language, Node, Parser, TreeCursor};
use walkdir::{DirEntry, WalkDir};

extern "C" {
    fn tree_sitter_java() -> Language;
}

#[derive(Debug, Default)]
pub struct DeclaredPackage {
    pub name: Option<String>,
    pub contained_identifiers: Vec<String>,
    pub files: Vec<String>,
}

impl DeclaredPackage {
    pub fn set_package_name(&mut self, name: String) {
        self.name = Some(name);
    }

    pub fn add_class_name(&mut self, class_name: String) {
        self.contained_identifiers.push(class_name);
    }

    pub fn add_file_name(&mut self, file_name: String) {
        self.files.push(file_name);
    }

    pub fn accum(&mut self, other: &DeclaredPackage) {
        if self.name == other.name {
            self.contained_identifiers
                .extend_from_slice(&other.contained_identifiers);
            self.contained_identifiers.sort();
            self.contained_identifiers.dedup();

            self.files.extend_from_slice(&other.files);
            self.files.sort();
            self.files.dedup();
        }
    }
}

fn print_tree(code: &String, cursor: &mut TreeCursor, indent: usize) {
    let node = cursor.node();
    if node.kind() == "local_variable_declaration" || node.kind() == "identifier" {
        println!(
            "{}{} [{}]",
            " ".repeat(indent),
            node.kind(),
            text_for_node(code, &node)
        );
    } else {
        println!("{}{}", " ".repeat(indent), node.kind());
    }
    if cursor.goto_first_child() {
        print_tree(code, cursor, indent + 2);
        cursor.goto_parent();
    }

    if cursor.goto_next_sibling() {
        print_tree(code, cursor, indent);
    }
}

fn print_tree_from_file(parser: &mut Parser, path: &Path) -> Result<()> {
    let code = std::fs::read_to_string(path)?;
    let tree = parser
        .parse(&code, None)
        .ok_or_else(|| Error::msg("Could not parse."))?;

    print_tree(&code, &mut tree.walk(), 0);

    Ok(())
}

fn text_for_node(code: &String, node: &Node) -> String {
    match node.utf8_text(code.as_bytes()) {
        Ok(t) => t.to_string(),
        Err(e) => {
            eprintln!("ERROR: {}", e);
            String::new()
        }
    }
}

#[allow(clippy::if_same_then_else)]
fn collect_identifier(code: &String, cursor: &mut TreeCursor, accum: &mut String) {
    if (cursor.node().kind() == "class_declaration"
        || cursor.node().kind() == "interface_declaration"
        || cursor.node().kind() == "package_declaration"
        || cursor.node().kind() == "enum_declaration")
        && !cursor.goto_first_child()
    {
        return;
    }

    if cursor.node().kind() == "scoped_identifier" {
        // We should be able to return the first scoped_identifier we find because the parsing of
        // identifiers is left-associative.
        accum.push_str(text_for_node(code, &cursor.node()).as_str());
        return;
    } else if cursor.node().kind() == "identifier" || cursor.node().kind() == "." {
        accum.push_str(text_for_node(code, &cursor.node()).as_str());
    } else if cursor.node().kind() == "marker_annotation" {
        return;
    } else if !accum.is_empty() {
        // If we've seen any identifier components then the first non-identifier component
        // implicitly ends the identifier, regardless of the validity of the syntax.
        return;
    }

    if cursor.goto_first_child() {
        collect_identifier(code, cursor, accum);
        cursor.goto_parent();
    }

    if cursor.goto_next_sibling() {
        collect_identifier(code, cursor, accum);
    }
}

fn collect_from_tree(code: &String, cursor: &mut TreeCursor, accum: &mut DeclaredPackage) {
    let node = cursor.node();

    if node.kind() == "package_declaration" {
        let mut identifier: String = String::new();
        collect_identifier(code, cursor, &mut identifier);
        if identifier.is_empty() {
            eprintln!("No identifier found in {}", node.kind());
        } else {
            accum.set_package_name(identifier.clone());
        }
        return;
    } else if node.kind() == "class_declaration"
        || node.kind() == "enum_declaration"
        || node.kind() == "interface_declaration"
    {
        let mut identifier: String = String::new();
        collect_identifier(code, cursor, &mut identifier);
        if identifier.is_empty() {
            eprintln!("No identifier found in {}", node.kind());
        } else {
            accum.add_class_name(identifier.clone());
        }
    }

    if cursor.goto_first_child() {
        collect_from_tree(code, cursor, accum);
        cursor.goto_parent();
    }

    if cursor.goto_next_sibling() {
        collect_from_tree(code, cursor, accum);
    }
}

fn collect_from_file(parser: &mut Parser, path: &Path) -> Result<DeclaredPackage> {
    let code = std::fs::read_to_string(path)?;
    let tree = parser
        .parse(&code, None)
        .ok_or_else(|| Error::msg("Could not parse."))?;

    let mut result = DeclaredPackage::default();
    collect_from_tree(&code, &mut tree.walk(), &mut result);
    if result.name.is_some() {
        if !result.contained_identifiers.is_empty() {
            result.add_file_name(path.to_str().unwrap().to_string());
        }
        Ok(result)
    } else {
        Err(Error::msg(format!(
            "No package declared in {}",
            path.to_str().unwrap()
        )))
    }
}

fn should_skip(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with('.') || s == "generated")
        .unwrap_or(false)
}

pub fn crawl_project(path: &Path) -> Result<Vec<DeclaredPackage>> {
    let mut parser = Parser::new();
    let lang = unsafe { tree_sitter_java() };
    parser.set_language(lang)?;

    let mut packages = HashMap::<String, DeclaredPackage>::new();

    let java_suffix = "java";
    for e in WalkDir::new(path)
        .into_iter()
        .filter_entry(|e| !should_skip(e))
    {
        // Silently ignore non-java files and files that lack UTF-8 names.
        let entry_path = e.ok().and_then(|entry| {
            let ext = entry.path().extension().and_then(|ext| ext.to_str())?;
            if ext == java_suffix {
                Some(entry.into_path())
            } else {
                None
            }
        });

        if let Some(path) = entry_path {
            let _: Result<_> = print_tree_from_file(&mut parser, &path);
            if let Ok(mut pkg) = collect_from_file(&mut parser, &path) {
                if let Some(pkg_name) = pkg.name.as_ref().cloned() {
                    if let Some(prev) = packages.get(&pkg_name) {
                        pkg.accum(prev);
                    }
                    packages.insert(pkg_name.clone(), pkg);
                }
            }
        }
    }

    // for (pkg_name, pkg) in packages {
    //     println!("{:?}", pkg);
    // }

    let result: Vec<DeclaredPackage> = packages.into_values().collect();

    Ok(result)
}
