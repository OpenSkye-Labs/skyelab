//! S0: launches run `git config` while other threads spawn long-lived agents. However the spawns interleave,
//! every call returns within a bound (a sibling that inherits the output pipe must not hold the call open).
#![cfg(unix)]

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn git_config_returns_while_siblings_spawn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stop = Arc::new(AtomicBool::new(false));
    let spawners: Vec<_> = (0..4)
        .map(|_| {
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut alive = std::collections::VecDeque::new();
                while !stop.load(Ordering::SeqCst) {
                    if let Ok(child) = Command::new("/bin/sleep")
                        .arg("30")
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                    {
                        alive.push_back(child);
                    }
                    if alive.len() > 8
                        && let Some(mut old) = alive.pop_front()
                    {
                        let _ = old.kill();
                        let _ = old.wait();
                    }
                }
                for mut c in alive {
                    let _ = c.kill();
                    let _ = c.wait();
                }
            })
        })
        .collect();
    let path = dir.path().to_owned();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for _ in 0..100 {
            let _ = skyelab_core::env::scoped_extra_headers(&path);
            if tx.send(()).is_err() {
                return;
            }
        }
    });
    let mut done = 0;
    while done < 100 {
        if rx.recv_timeout(Duration::from_secs(5)).is_err() {
            stop.store(true, Ordering::SeqCst);
            panic!("call {} of git config hung past 5 s", done + 1);
        }
        done += 1;
    }
    stop.store(true, Ordering::SeqCst);
    for s in spawners {
        s.join().expect("spawner");
    }
}
