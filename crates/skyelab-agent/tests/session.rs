#![cfg(unix)]
mod common;

use common::start_in;
use skyelab_agent::{Event, Launch, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn fake_bin() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh")
}

fn bound(dir: &Path, config_dir: &Path) -> AccountsConfig {
    AccountsConfig {
        accounts: vec![Account {
            id: "acme".into(),
            label: "Acme".into(),
            config_dir: config_dir.into(),
        }],
        bindings: vec![Binding::new(dir.into(), "acme".into())],
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("skyelab-agent-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    std::fs::canonicalize(&d).expect("canonicalize temp dir")
}

fn next(rx: &flume::Receiver<Event>) -> Event {
    rx.recv_timeout(Duration::from_secs(10))
        .expect("event within 10s")
}

/// Reads events until the fake script's env report.
fn fake_env(rx: &flume::Receiver<Event>) -> serde_json::Value {
    loop {
        if let Event::Unknown(v) = next(rx)
            && v["type"] == "fake_env"
        {
            return v;
        }
    }
}

#[test]
fn unbound_folder_errors() {
    let dir = temp_dir("unbound");
    let account = Account {
        id: "acme".into(),
        label: "Acme".into(),
        config_dir: dir.join("account"),
    };
    let launch = Launch {
        account: &account,
        dir: &dir,
        resume: None,
        prompt: "hi",
    };
    let err = start_with_bin(&fake_bin(), &AccountsConfig::default(), &launch)
        .err()
        .expect("unbound folder must not start");
    assert!(err.to_string().contains("not bound"), "{err:#}");
}

const CHILD_MARKER: &str = "SKYELAB_AGENT_TEST_CHILD";

/// Run by `child_gets_bound_config_dir` in a copy of this test binary whose env has a wrong CLAUDE_CONFIG_DIR.
#[test]
fn env_check_in_child() {
    if std::env::var_os(CHILD_MARKER).is_none() {
        return;
    }
    assert_eq!(
        std::env::var("CLAUDE_CONFIG_DIR").as_deref(),
        Ok("/wrong/parent/value")
    );
    let dir = temp_dir("env");
    let account_dir = dir.join("account");
    let cfg = bound(&dir, &account_dir);
    let session = start_in(&fake_bin(), &cfg, &dir, "hi").expect("start");
    let env = fake_env(session.events());
    assert_eq!(env["api_key"], "", "parent ANTHROPIC_API_KEY leaked");
    assert_eq!(env["config_dir"], account_dir.to_str().expect("utf-8 path"));
    assert_eq!(env["cwd"], dir.to_str().expect("utf-8 path"));
    let args = env["args"].as_str().expect("args");
    for flag in [
        "-p",
        "--output-format stream-json",
        "--input-format stream-json",
        "--verbose",
    ] {
        assert!(args.contains(flag), "missing {flag} in {args}");
    }
}

#[test]
fn child_gets_bound_config_dir() {
    let out = std::process::Command::new(std::env::current_exe().expect("test binary path"))
        .args(["env_check_in_child", "--exact", "--nocapture"])
        .env(CHILD_MARKER, "1")
        .env("CLAUDE_CONFIG_DIR", "/wrong/parent/value")
        .env("ANTHROPIC_API_KEY", "parent-key")
        .output()
        .expect("run child test");
    let log =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{log}");
    assert!(log.contains("1 passed"), "{log}");
}

#[test]
fn send_and_kill() {
    let dir = temp_dir("send");
    let cfg = bound(&dir, &dir.join("account"));
    let mut session = start_in(&fake_bin(), &cfg, &dir, "first").expect("start");
    let rx = session.events().clone();
    assert!(matches!(next(&rx), Event::Init { .. }));
    fake_env(&rx);
    // The fake echoes stdin, so the prompt and each follow-up come back as user lines.
    let echoed = |rx: &flume::Receiver<Event>| match next(rx) {
        Event::Unknown(v) => v["message"]["content"].clone(),
        other => panic!("expected echoed user line, got {other:?}"),
    };
    assert_eq!(echoed(&rx), "first");
    session.send_user_message("second").expect("send");
    assert_eq!(echoed(&rx), "second");
    session.kill().expect("kill");
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(10)).map(|_| ()),
        Err(flume::RecvTimeoutError::Disconnected),
        "stream ends after kill, even with a grandchild holding stdout"
    );
}
