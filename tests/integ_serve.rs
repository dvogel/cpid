use cpid;

use std::io::Read;
use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{bail, Error, Result};

extern crate serde_json;
extern crate sled;

fn start_serve() -> std::thread::JoinHandle<()> {
    thread::spawn(move || {
        let db = sled::open("test.sled").expect("writable database file.");
        cpid::serve::serve_unix(&db, String::from("test.socket"));
        std::fs::remove_file("test.socket");
    })
}

fn read_reply(src: &mut dyn Read, buf: &mut String) {
    buf.clear();
    eprintln!("SLEEPING 10ms");
    std::thread::sleep(Duration::from_millis(10));
    match src.read_to_string(buf) {
        Ok(cnt) => {
            if cnt == 0 {
                eprintln!("READ 0 BYTES, RETRYING...");
                read_reply(src, buf);
            } else {
                return;
            }
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                eprintln!("PARTIAL READ? {}", buf);
                eprintln!("READ WOULD BLOCK, RETRYING...");
                read_reply(src, buf);
            } else {
                eprintln!("READ ERROR: {}", e);
            }
        }
    }
}

#[test]
fn enumerate_sample_project_classpath() -> Result<()> {
    let serve_thread = start_serve();
    std::thread::sleep(Duration::from_millis(1000));
    let mut client_socket = match UnixStream::connect("test.socket") {
        Ok(sock) => sock,
        Err(e) => {
            panic!("SOCKET FAILURE: {}", e);
        }
    };

    let reply_stream = client_socket.try_clone().expect("Socket clone failed.");

    let mut replies = serde_json::Deserializer::from_reader(std::io::BufReader::new(&reply_stream))
        .into_iter::<serde_json::Value>();

    let classpath = std::fs::read_to_string("vim-cpid/test-app/pom.xml.classpath-cache")?;
    let reindex_cmd = format!(
        r#"[1, {{
            "type":"ReindexClasspathCmd",
            "index_name": "testidx",
            "archive_source": "{}"
        }}]"#,
        classpath
    );
    client_socket.write_all(&reindex_cmd.as_bytes());
    client_socket.flush();
    let reindex_reply = replies
        .next()
        .expect("Reply read failure.")
        .expect("JSON deserialization failure.");
    let expected_reindex_reply: serde_json::Value =
        serde_json::from_str::<serde_json::Value>(r#"[1,{"type":"NullResponse"}]"#)?;
    assert_eq!(expected_reindex_reply, reindex_reply);

    let query_msg = r#"[2, {
            "type":"ClassQuery",
            "index_name": "testidx",
            "class_name": "Timeout"
        }]"#;
    client_socket.write_all(&query_msg.as_bytes());
    client_socket.flush();

    let qry_reply = replies
        .next()
        .expect("Reply read failure.")
        .expect("JSON deserialization failure.");
    let expected_qry_reply: serde_json::Value = serde_json::from_str::<serde_json::Value>(
        r#"[2,{"type":"ClassQueryResponse","results":{"Timeout":["org.junit.rules","org.openjdk.jmh.annotations"]}}]"#,
    )?;
    assert_eq!(expected_qry_reply, qry_reply);

    client_socket.write_all(r#"[3, {"type":"ShutdownCmd"}]"#.as_bytes());
    serve_thread.join();
    Ok(())
}
