//! Every child handle from `skyelab_core::process` reaps its process when dropped: a running one is killed first,
//! a detached one is reaped once it exits. An unreaped child stays a zombie until this process exits, and zombies count
//! toward the per-user process limit (spawns then fail with EAGAIN, as on GitHub's macOS runners). Its own binary, so
//! no other test's children are counted.
#![cfg(unix)]

use skyelab_core::process::{self, Captured};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// This process's children, as (pid, zombie).
fn children() -> Vec<(u32, bool)> {
    let me = std::process::id().to_string();
    let out = Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,stat="])
        .output()
        .expect("ps runs");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (pid, ppid, stat) = (f.next()?, f.next()?, f.next()?);
            (ppid == me).then(|| (pid.parse().ok(), stat.starts_with('Z')))
        })
        .filter_map(|(pid, z)| Some((pid?, z)))
        .collect()
}

/// Waits (bounded) until none of `pids` is a child of this process any more, live or zombie.
fn all_reaped(pids: &[u32]) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if !children().iter().any(|(p, _)| pids.contains(p)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn sleeper(secs: &str) -> Command {
    let mut cmd = Command::new("/bin/sleep");
    cmd.arg(secs).stdin(Stdio::null());
    cmd
}

#[test]
fn dropped_handles_kill_and_reap_their_children() {
    let mut pids = Vec::new();
    // Still running when dropped: killed, then reaped.
    for _ in 0..3 {
        let child = process::spawn(&mut sleeper("30")).unwrap();
        pids.push(child.id());
    }
    // Exited but never waited (the shape of the frozen-config leak): reaped.
    let exited = process::spawn(Command::new("/usr/bin/true").stdin(Stdio::null())).unwrap();
    pids.push(exited.id());
    std::thread::sleep(Duration::from_millis(200));
    drop(exited);
    // A Captured dropped on an error path, without `wait` or `output`.
    let captured = Captured::spawn(&mut sleeper("30"), None).unwrap();
    pids.push(captured.child.id());
    drop(captured);
    assert!(all_reaped(&pids), "left behind: {:?}", children());
}

#[test]
fn a_detached_child_runs_on_and_is_reaped_when_it_exits() {
    let child = process::spawn(&mut sleeper("1")).unwrap();
    let pid = child.id();
    child.detach();
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        children().contains(&(pid, false)),
        "a detached child keeps running: {:?}",
        children()
    );
    assert!(all_reaped(&[pid]), "left behind: {:?}", children());
}
