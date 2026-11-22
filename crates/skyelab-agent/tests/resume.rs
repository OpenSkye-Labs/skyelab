#![cfg(unix)]
//! Resuming a session, the spawn-time account re-check, and a Stop that never waits on stdin.
//! Every fake here is a shell script; nothing runs the real `claude`.

use skyelab_agent::{Event, Launch, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("skyelab-resume-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("create temp dir");
    std::fs::canonicalize(&d).expect("canonicalize temp dir")
}

fn account(id: &str, dir: &Path) -> Account {
    Account {
        id: id.into(),
        label: id.into(),
        config_dir: dir.join(id),
    }
}

/// Accounts "a" and "b", with `dir` bound to `bound_to`.
fn config(dir: &Path, bound_to: &str) -> AccountsConfig {
    AccountsConfig {
        accounts: vec![account("a", dir), account("b", dir)],
        bindings: vec![Binding::new(dir.into(), bound_to.into())],
    }
}

fn next_unknown(rx: &flume::Receiver<Event>, ty: &str) -> serde_json::Value {
    loop {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Event::Unknown(v)) if v["type"] == ty => return v,
            Ok(_) => {}
            Err(e) => panic!("no {ty} line: {e}"),
        }
    }
}

/// A fake that records that it ran, so a refused launch can be shown to start nothing.
fn marker_bin(dir: &Path) -> PathBuf {
    let bin = dir.join("marker-claude.sh");
    std::fs::write(
        &bin,
        format!("#!/bin/sh\ntouch '{}'\n", dir.join("ran").display()),
    )
    .expect("write fake");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    bin
}

#[test]
fn resume_passes_flag_and_id() {
    let dir = temp_dir("flag");
    let cfg = config(&dir, "a");
    let a = cfg.resolve(&dir).cloned().expect("bound");
    let launch = Launch {
        account: &a,
        dir: &dir,
        resume: Some("sess-1"),
        prompt: "again",
        mcp_config: None,
    };
    let session = start_with_bin(&fixture("fake-claude.sh"), &cfg, &launch).expect("start");
    let env = next_unknown(session.events(), "fake_env");
    let args = env["args"].as_str().expect("args");
    assert!(args.contains("--resume sess-1"), "{args}");
    assert_eq!(env["config_dir"], dir.join("a").to_str().expect("utf-8"));
}

#[test]
fn resume_rejects_flag_like_id() {
    let dir = temp_dir("badid");
    let cfg = config(&dir, "a");
    let a = cfg.resolve(&dir).cloned().expect("bound");
    for id in ["--dangerously-skip-permissions", "-x", "a b", "", "a;b"] {
        let launch = Launch {
            account: &a,
            dir: &dir,
            resume: Some(id),
            prompt: "hi",
            mcp_config: None,
        };
        assert!(
            start_with_bin(&marker_bin(&dir), &cfg, &launch).is_err(),
            "{id:?}"
        );
    }
    assert!(!dir.join("ran").exists(), "a refused resume started claude");
}

/// The caller resolved "a" and recorded it; by spawn time the folder is bound to "b".
#[test]
fn binding_changed_between_plan_and_spawn_aborts() {
    let dir = temp_dir("toctou");
    let planned = config(&dir, "a").resolve(&dir).cloned().expect("bound");
    let launch = Launch {
        account: &planned,
        dir: &dir,
        resume: None,
        prompt: "hi",
        mcp_config: None,
    };
    let err = start_with_bin(&marker_bin(&dir), &config(&dir, "b"), &launch)
        .err()
        .expect("a rebound folder must not start");
    assert!(err.to_string().contains("now bound to 'b'"), "{err:#}");
    let unbound = AccountsConfig {
        bindings: vec![],
        ..config(&dir, "a")
    };
    assert!(start_with_bin(&marker_bin(&dir), &unbound, &launch).is_err());
    assert!(
        !dir.join("ran").exists(),
        "an aborted launch started claude"
    );
}

fn group_alive(pgid: i64) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", "--", &format!("-{pgid}")])
        .stderr(std::process::Stdio::null())
        .status()
        .expect("run kill")
        .success()
}

#[test]
fn kill_does_not_wait_behind_a_blocked_write() {
    let dir = temp_dir("blocked");
    let cfg = config(&dir, "a");
    let a = cfg.resolve(&dir).cloned().expect("bound");
    let launch = Launch {
        account: &a,
        dir: &dir,
        resume: None,
        prompt: "hi",
        mcp_config: None,
    };
    let mut session = start_with_bin(&fixture("no-stdin.sh"), &cfg, &launch).expect("start");
    let pgid = next_unknown(session.events(), "fake_pid")["pid"]
        .as_i64()
        .expect("pid");
    // Far more than a pipe holds, so the writer blocks and the queue fills; sending must not block.
    let big = "x".repeat(256 << 10);
    let started = Instant::now();
    let refused = (0..100).any(|_| session.send_user_message(&big).is_err());
    assert!(refused, "a full queue reports an error");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "sending blocked"
    );

    let started = Instant::now();
    session.kill().expect("kill");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "kill waited {:?}",
        started.elapsed()
    );
    let gone = Instant::now();
    while group_alive(pgid) {
        assert!(
            gone.elapsed() < Duration::from_secs(5),
            "process group {pgid} survived kill"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
