//! A child starts with no blocked signals, whatever the spawning thread blocks. std copies the caller's mask into the
//! child, and GPUI's background threads block SIGCHLD: Codex then never saw its hook processes exit and hung.
#![cfg(unix)]

use nix::sys::signal::{SigSet, SigmaskHow, Signal, pthread_sigmask};
use std::process::{Command, Stdio};

const CHILD: &str = "SKYELAB_SIGMASK_CHILD";

fn blocked() -> SigSet {
    let mut now = SigSet::empty();
    pthread_sigmask(SigmaskHow::SIG_BLOCK, None, Some(&mut now)).expect("reading the mask");
    now
}

/// The child: says whether its main thread starts with SIGCHLD blocked.
#[test]
fn child_reports_its_mask() {
    if std::env::var_os(CHILD).is_some() {
        let word = if blocked().contains(Signal::SIGCHLD) {
            "blocked"
        } else {
            "clear"
        };
        println!("sigchld={word}");
    }
}

fn child() -> Command {
    let mut cmd = Command::new(std::env::current_exe().expect("test binary"));
    cmd.args([
        "--exact",
        "child_reports_its_mask",
        "--nocapture",
        "--test-threads=1",
    ])
    .env(CHILD, "1")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    cmd
}

/// Runs `spawn` on a fresh thread that blocks SIGCHLD; returns what the child said and whether that thread still
/// blocks SIGCHLD afterwards.
fn from_a_blocking_thread(spawn: fn(&mut Command) -> std::process::Output) -> (String, bool) {
    std::thread::spawn(move || {
        let mut only = SigSet::empty();
        only.add(Signal::SIGCHLD);
        pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&only), None).expect("blocking SIGCHLD");
        let out = spawn(&mut child());
        let said = String::from_utf8_lossy(&out.stdout).into_owned();
        (said, blocked().contains(Signal::SIGCHLD))
    })
    .join()
    .expect("spawning thread")
}

#[test]
fn a_child_spawned_from_a_thread_that_blocks_sigchld_starts_with_it_clear() {
    let (said, kept) = from_a_blocking_thread(|cmd| {
        // process::output goes through process::spawn, as every agent launch does.
        skyelab_core::process::output(cmd, None).expect("child output")
    });
    assert!(said.contains("sigchld=clear"), "child said: {said:?}");
    assert!(
        kept,
        "the spawning thread's own mask must come back after the spawn"
    );
}

#[test]
fn a_spawn_run_under_exclusive_starts_with_sigchld_clear_too() {
    // The terminal's PTY spawns this way, not through process::spawn.
    let (said, kept) = from_a_blocking_thread(|cmd| {
        skyelab_core::process::exclusive(|| cmd.spawn())
            .expect("spawned")
            .wait_with_output()
            .expect("child output")
    });
    assert!(said.contains("sigchld=clear"), "child said: {said:?}");
    assert!(
        kept,
        "the spawning thread's own mask must come back after the spawn"
    );
}
