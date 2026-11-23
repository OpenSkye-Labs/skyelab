//! Children start one at a time. On macOS std makes a pipe close-on-exec only after creating it (the exec-status
//! pipe of every spawn that forks, and piped stdio), so a child started by another thread in that gap keeps the
//! write end: the first spawn then blocks until that child (an agent, hours) exits. Holding one lock across every
//! spawn closes the gap; this checks the lock really excludes a second spawn.
#![cfg(unix)]

use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use skyelab_core::process;

#[test]
fn a_spawn_waits_while_another_is_starting() {
    let (tx, rx) = mpsc::channel();
    process::exclusive(|| {
        std::thread::spawn(move || {
            let started = process::spawn(Command::new("/usr/bin/true").stdout(Stdio::null()))
                .and_then(|mut c| c.wait());
            let _ = tx.send(started.is_ok());
        });
        assert!(
            rx.recv_timeout(Duration::from_millis(500)).is_err(),
            "a child started while another spawn held the lock"
        );
    });
    assert!(
        rx.recv_timeout(Duration::from_secs(10))
            .expect("the spawn runs once the lock is free"),
        "the spawn failed"
    );
}
