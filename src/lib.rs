#![allow(unused_imports)]
#![allow(unused_variables)]

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::thread;
use std::time;

extern crate serde_derive;
extern crate serde_json;
extern crate sled;

use anyhow::{bail, Result};
use serde_derive::Serialize;
use zip::read::ZipArchive;
use zip::result::ZipResult;

pub mod indexes;
pub mod proto;
pub mod serve;
