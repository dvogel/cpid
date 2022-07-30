#![allow(unused_imports)]
#![allow(unused_variables)]

use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

extern crate sled;

use anyhow::{bail, Result};

use crate::proto;

fn serve_accept_loop(
    db: &sled::Db,
    socket_path: String,
    shutdown_cond: Arc<AtomicBool>,
) -> Result<()> {
    let listener = UnixListener::bind(&socket_path)?;
    println!("Listening on {}", socket_path);
    for client in listener.incoming() {
        match client {
            Ok(stream) => {
                let db1 = db.clone();
                let shutdown_cond1 = shutdown_cond.clone();
                thread::spawn(move || {
                    proto::handle_client(db1, stream, shutdown_cond1);
                });
            }
            Err(e) => {
                eprintln!("Failed to handle incoming client: {}", e);
            }
        }
    }
    Ok(())
}

pub fn serve_unix(db: &sled::Db, socket_path: String) -> Result<()> {
    let mut shutdown_cond = Arc::new(AtomicBool::new(false));

    // The thread below will call accept() for us. Since that will block and is reentrant the we
    // cannot rely on it being interrupted. Therefore we will wait for the signal here and remove
    // the socket file out from under the accept() thread during shutdown.
    let shutdown_cond1 = shutdown_cond.clone();
    let path1 = socket_path.clone();
    let db1 = db.clone();
    thread::spawn(move || serve_accept_loop(&db1, path1, shutdown_cond1));
    while !shutdown_cond.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(100));
    }

    std::fs::remove_file(socket_path)?;
    Ok(())
}
