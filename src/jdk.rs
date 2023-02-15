#![allow(unused_imports)]
#![allow(unused_variables)]

use std::io::Read;

pub fn is_jimage_file(path: &str) -> bool {
    let mut buf = [0; 4];
    std::fs::File::open(path)
        .and_then(|mut inf| inf.read_exact(&mut buf))
        .map(|()| buf[0] == 0xda && buf[1] == 0xda && buf[2] == 0xfe && buf[3] == 0xca)
        .unwrap_or(false)
}
