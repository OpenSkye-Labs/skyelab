//! Sharing turned off and on quickly binds the same port again while other threads start children (git, agents).
//! On macOS std makes the listener close-on-exec only after creating it, so a child forked in that gap would keep
//! the listening socket, and the next bind fails with "Address already in use" until that child exits.
#![cfg(unix)]

mod common;

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use common::*;
use skyelab_daemon::Daemon;

#[test]
fn quick_restarts_rebind_while_children_spawn() {
    let stop = Arc::new(AtomicBool::new(false));
    let spawners: Vec<_> = (0..4)
        .map(|_| {
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let mut child = Command::new("/bin/sleep");
                    child.arg("0.3").stdin(Stdio::null());
                    // Reaped off-thread so this one keeps forking.
                    if let Ok(mut c) = skyelab_core::process::spawn(&mut child) {
                        std::thread::spawn(move || c.wait());
                    }
                }
            })
        })
        .collect();
    let backend = Arc::new(Counting::default());
    let mut daemon = start(
        Arc::new(FakeTailnet::default()),
        backend.clone(),
        test_config(),
    );
    let port = daemon.local_addrs()[0].port();
    let mut config = test_config();
    config.port = port;
    for n in 0..200 {
        drop(daemon);
        daemon = match Daemon::start(
            config.clone(),
            Arc::new(FakeTailnet::default()),
            backend.clone(),
        ) {
            Ok(d) => d,
            Err(e) => {
                stop.store(true, Ordering::SeqCst);
                panic!("restart {n} on port {port}: {e:#}");
            }
        };
    }
    stop.store(true, Ordering::SeqCst);
    for s in spawners {
        s.join().expect("spawner");
    }
}
