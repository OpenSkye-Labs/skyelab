//! `process::output` returns when the child exits, even while a process it left behind still holds its
//! stdout or stderr, and it hands `input` to the child's stdin.
#![cfg(unix)]

use skyelab_core::process::output;
use std::process::{Command, Output};
use std::sync::mpsc;
use std::time::Duration;

fn bounded(mut cmd: Command, input: Option<&'static [u8]>) -> Output {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(output(&mut cmd, input));
    });
    rx.recv_timeout(Duration::from_secs(60))
        .expect("returned within 60 s")
        .expect("ran")
}

#[test]
fn a_grandchild_holding_stdout_and_stderr_does_not_block() {
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", "sleep 30 & echo out; echo err >&2; exit 3"]);
    let out = bounded(cmd, None);
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(out.stdout, b"out\n");
    assert_eq!(out.stderr, b"err\n");
}

#[test]
fn input_reaches_stdin_and_a_held_stdout_still_returns() {
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", "sleep 30 & cat"]);
    let out = bounded(cmd, Some(b"hello\nworld\n"));
    assert!(out.status.success());
    assert_eq!(out.stdout, b"hello\nworld\n");
}

#[test]
fn without_input_stdin_is_empty() {
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", "cat; echo done"]);
    let out = bounded(cmd, None);
    assert_eq!(out.stdout, b"done\n");
}
