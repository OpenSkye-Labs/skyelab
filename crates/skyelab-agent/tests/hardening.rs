#![cfg(unix)]
//! What an untrusted repo or a hostile parent env can and can't do to a session, and the
//! process lifecycle. Every fake here is a shell script; nothing runs the real `claude`.
mod common;

use common::start_in;
use skyelab_agent::{Event, Launch, start};
use skyelab_core::{Account, AccountsConfig, Binding};
use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn bound(dir: &Path, config_dir: &Path) -> AccountsConfig {
    AccountsConfig {
        accounts: vec![Account {
            id: "acme".into(),
            label: "Acme".into(),
            config_dir: config_dir.into(),
            provider: Default::default(),
            gh_config_dir: None,
            ssh_agent: false,
        }],
        bindings: vec![Binding::new(dir.into(), "acme".into())],
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("skyelab-hardening-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("create temp dir");
    std::fs::canonicalize(&d).expect("canonicalize temp dir")
}

fn script(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().expect("script has a parent")).expect("mkdir");
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("write script");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

fn next(rx: &flume::Receiver<Event>) -> Event {
    rx.recv_timeout(Duration::from_secs(10))
        .expect("event within 10s")
}

/// The next unmodelled line (of type `ty`, if given), as raw JSON.
fn next_unknown(rx: &flume::Receiver<Event>, ty: Option<&str>) -> serde_json::Value {
    loop {
        if let Event::Unknown(v) = next(rx)
            && ty.is_none_or(|t| v["type"] == t)
        {
            return v;
        }
    }
}

const CHILD: &str = "SKYELAB_HARDENING_CHILD";
const DIR: &str = "SKYELAB_HARDENING_DIR";

/// Runs `test` in a copy of this binary with only `env` and the child marker (nothing inherited).
fn run_child(test: &str, env: &[(&str, &str)]) {
    let out = std::process::Command::new(std::env::current_exe().expect("test binary path"))
        .args([test, "--exact", "--nocapture", "--test-threads=1"])
        .env_clear()
        .env(CHILD, "1")
        .envs(env.iter().copied())
        .output()
        .expect("run child test");
    let log =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{log}");
    assert!(log.contains("1 passed"), "{log}");
}

fn in_child() -> bool {
    std::env::var_os(CHILD).is_some()
}

// Project settings and .mcp.json are untrusted, so only the account's own settings load.
#[test]
fn ignores_project_settings_and_mcp() {
    let dir = temp_dir("args");
    let session = start_in(
        &fixture("fake-claude.sh"),
        &bound(&dir, &dir.join("acct")),
        &dir,
        "hi",
    )
    .expect("start");
    let env = next_unknown(session.events(), Some("fake_env"));
    let args = env["args"].as_str().expect("args");
    assert!(args.contains("--setting-sources user"), "{args}");
    assert!(args.contains("--strict-mcp-config"), "{args}");
}

// The child env is an allowlist plus the bound account, nothing else.
#[test]
fn env_allowlist_in_child() {
    if !in_child() {
        return;
    }
    let dir = PathBuf::from(std::env::var(DIR).expect("dir"));
    let acct = dir.join("acct");
    let session =
        start_in(&fixture("fake-claude.sh"), &bound(&dir, &acct), &dir, "hi").expect("start");
    let env = next_unknown(session.events(), Some("fake_env"));
    let keys: BTreeSet<&str> = env["env_keys"]
        .as_str()
        .expect("env_keys")
        .split_whitespace()
        .filter(|k| !["PWD", "OLDPWD", "SHLVL", "_"].contains(k)) // /bin/sh sets these itself
        .filter(|k| !k.starts_with("GIT_CONFIG_")) // the account's git credentials (agent_git_credentials.rs)
        .collect();
    let want: BTreeSet<&str> = [
        "PATH",
        "HOME",
        "LANG",
        "LC_ALL",
        "CLAUDE_CONFIG_DIR",
        "GIT_TERMINAL_PROMPT",
    ]
    .into();
    assert_eq!(keys, want);
    assert_eq!(env["config_dir"], acct.to_str().expect("utf-8"));
}

#[test]
fn env_allowlist() {
    let dir = temp_dir("env");
    let dir = dir.to_str().expect("utf-8");
    run_child(
        "env_allowlist_in_child",
        &[
            (DIR, dir),
            ("PATH", "/usr/bin:/bin"),
            ("HOME", dir),
            ("LANG", "C"),
            ("LC_ALL", "C"),
            ("SKYELAB_UNRELATED", "1"),
            ("CLAUDE_CONFIG_DIR", "/wrong/parent/value"),
            ("ANTHROPIC_API_KEY", "x"),
            ("CLAUDE_CODE_USE_FOUNDRY", "1"),
            ("ANTHROPIC_FOUNDRY_API_KEY", "x"),
            ("ANTHROPIC_CUSTOM_HEADERS", "x: y"),
            ("ANTHROPIC_AUTH_TOKEN", "x"),
            ("CLAUDE_CODE_OAUTH_TOKEN", "x"),
            ("ANTHROPIC_BASE_URL", "http://evil.invalid"),
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
            ("CLAUDE_CODE_USE_VERTEX", "1"),
        ],
    );
}

// A relative PATH entry would resolve inside the untrusted project cwd.
#[test]
fn path_lookup_in_child() {
    if !in_child() {
        return;
    }
    let dir = PathBuf::from(std::env::var(DIR).expect("dir"));
    let cfg = bound(&dir, &dir.join("acct"));
    let account = cfg.resolve(&dir).cloned().expect("bound");
    let launch = Launch {
        account: &account,
        dir: &dir,
        resume: None,
        prompt: "hi",
        mcp_config: None,
    };
    let session = start(&cfg, &launch).expect("start");
    let which = next_unknown(session.events(), Some("which"));
    assert_eq!(which["bin"], "abs");
    assert_eq!(which["path"], dir.join("absbin").to_str().expect("utf-8"));
    drop(session);
    assert!(
        !dir.join("ran-relative").exists(),
        "ran the planted relbin/claude"
    );
    // Only a relative PATH entry has this name: not found, never run.
    assert!(start_in(Path::new("claude-only-relative"), &cfg, &dir, "hi").is_err());
    assert!(
        !dir.join("ran-relative").exists(),
        "ran the planted relbin/claude-only-relative"
    );
}

#[test]
fn path_lookup_skips_relative_entries() {
    let dir = temp_dir("path");
    let planted = ": > ran-relative; echo '{\"type\":\"which\",\"bin\":\"relative\"}'";
    script(&dir.join("relbin/claude"), planted);
    script(&dir.join("relbin/claude-only-relative"), planted);
    script(
        &dir.join("absbin/claude"),
        "echo \"{\\\"type\\\":\\\"which\\\",\\\"bin\\\":\\\"abs\\\",\\\"path\\\":\\\"$PATH\\\"}\"\nwhile read -r l; do :; done",
    );
    let path = format!("relbin:{}", dir.join("absbin").display());
    run_child(
        "path_lookup_in_child",
        &[(DIR, dir.to_str().expect("utf-8")), ("PATH", &path)],
    );
}

#[test]
fn relative_bin_with_dirs_is_rejected() {
    let dir = temp_dir("relpath");
    script(&dir.join("sub/claude"), ": > ran-relative");
    let res = start_in(
        Path::new("sub/claude"),
        &bound(&dir, &dir.join("a")),
        &dir,
        "hi",
    );
    // Refused while resolving the binary, before any spawn, so nothing can have run.
    let err = format!("{:#}", res.err().expect("a relative bin path is refused"));
    assert!(
        err.contains("must be an absolute path or a bare name"),
        "{err}"
    );
    assert!(!dir.join("ran-relative").exists());
}

// A reaped child is never signalled again, and its exit status is visible.
#[test]
fn kill_once_in_child() {
    if !in_child() {
        return;
    }
    let dir = PathBuf::from(std::env::var(DIR).expect("dir"));
    let mut session = start_in(
        &fixture("fake-claude.sh"),
        &bound(&dir, &dir.join("a")),
        &dir,
        "hi",
    )
    .expect("start");
    next_unknown(session.events(), Some("fake_env"));
    assert_eq!(session.try_wait().expect("try_wait"), None, "still running");
    session.kill().expect("kill");
    assert!(session.try_wait().expect("try_wait").is_some());
    session.kill().expect("second kill is a no-op");
    drop(session);
    let log = std::fs::read_to_string(dir.join("kill.log")).unwrap_or_default();
    assert_eq!(log.lines().count(), 1, "group kill ran {log:?}");
}

#[test]
fn reaped_child_is_not_signalled_again() {
    let dir = temp_dir("kill");
    let log = dir.join("kill.log");
    // The shell builtin does the real kill, so the group still dies.
    script(
        &dir.join("killbin/kill"),
        &format!("echo \"$*\" >> '{}'\nkill \"$@\"", log.display()),
    );
    let path = format!("{}:/usr/bin:/bin", dir.join("killbin").display());
    run_child(
        "kill_once_in_child",
        &[(DIR, dir.to_str().expect("utf-8")), ("PATH", &path)],
    );
}

#[test]
fn exit_status_of_self_exit() {
    let dir = temp_dir("exit");
    let mut session = start_in(
        &fixture("exit-3.sh"),
        &bound(&dir, &dir.join("a")),
        &dir,
        "hi",
    )
    .expect("start");
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(s) = session.try_wait().expect("try_wait") {
            break s;
        }
        assert!(Instant::now() < deadline, "child never exited");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(3));
}

// One huge line is capped, not buffered whole, and the stream carries on after it.
#[test]
fn oversized_line_is_capped() {
    let dir = temp_dir("long");
    let session = start_in(
        &fixture("long-line.sh"),
        &bound(&dir, &dir.join("a")),
        &dir,
        "hi",
    )
    .expect("start");
    match next(session.events()) {
        Event::Unknown(serde_json::Value::String(s)) => assert_eq!(s.len(), 16 << 20),
        other => panic!("expected a truncated line, got {other:?}"),
    }
    assert_eq!(next_unknown(session.events(), None)["type"], "after");
}

// The stream-json input format the SDK sends.
#[test]
fn message_and_interrupt_format() {
    let dir = temp_dir("fmt");
    let mut session = start_in(
        &fixture("fake-claude.sh"),
        &bound(&dir, &dir.join("a")),
        &dir,
        "first",
    )
    .expect("start");
    let rx = session.events().clone();
    next_unknown(&rx, Some("fake_env"));
    let user = next_unknown(&rx, None);
    assert_eq!(user["type"], "user");
    assert_eq!(user["message"]["content"], "first");
    assert_eq!(
        user.get("parent_tool_use_id"),
        Some(&serde_json::Value::Null)
    );
    assert_eq!(user["session_id"], "");
    for id in ["skyelab-1", "skyelab-2"] {
        session.interrupt().expect("interrupt");
        let req = next_unknown(&rx, None);
        assert_eq!(req["type"], "control_request");
        assert_eq!(req["request_id"], id);
        assert_eq!(req["request"]["subtype"], "interrupt");
    }
}
