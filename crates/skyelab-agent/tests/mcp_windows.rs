//! The named-pipe transport on Windows: a per-run random pipe, the proxy round trip, a revoked grant, stale runs.
#![cfg(windows)]

use serde_json::{Value, json};
use skyelab_agent::mcp::{Caller, Host, Proxy, Server, Tool, remove_stale_runs, run_proxy};
use std::sync::Arc;

struct Nop;
impl Host for Nop {
    fn call(&self, _: &Caller, _: Tool) -> Result<Value, String> {
        Ok(json!({"ok": true}))
    }
}

fn start(tmp: &tempfile::TempDir, name: &str) -> Server {
    Server::start(&tmp.path().join(name), Arc::new(Nop)).expect("start")
}

fn caller() -> Caller {
    Caller {
        thread_id: 7,
        account_id: "acme".into(),
    }
}

fn proxy() -> Proxy {
    Proxy {
        program: r"C:\skyelab.exe".into(),
        args: vec!["--mcp-proxy".into()],
    }
}

#[test]
fn each_server_gets_its_own_random_pipe() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (a, b) = (start(&tmp, "a"), start(&tmp, "b"));
    let prefix = format!(r"\\.\pipe\skyelab-{}-", std::process::id());
    for s in [&a, &b] {
        let name = s.socket_path().to_str().expect("utf-8");
        let random = name
            .strip_prefix(&prefix)
            .expect("a pipe named after this run");
        assert_eq!(random.len(), 64, "{name}");
        assert!(random.chars().all(|c| c.is_ascii_hexdigit()), "{name}");
    }
    assert_ne!(a.socket_path(), b.socket_path());
}

#[test]
fn the_grant_names_the_pipe_and_the_proxy_round_trips() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let server = start(&tmp, "run");
    let grant = server.grant(caller(), &proxy()).expect("grant");
    let config: Value =
        serde_json::from_str(&std::fs::read_to_string(grant.config_path()).expect("read"))
            .expect("json");
    assert_eq!(
        config["mcpServers"]["skyelab"]["env"]["SKYELAB_MCP_SOCKET"],
        server.socket_path().to_str().expect("utf-8")
    );
    let input = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "list_projects", "arguments": {}}}),
    ]
    .iter()
    .map(|v| format!("{v}\n"))
    .collect::<String>();
    let mut out = Vec::new();
    run_proxy(
        server.socket_path(),
        grant.token(),
        std::io::Cursor::new(input.into_bytes()),
        &mut out,
    )
    .expect("the proxy returns once both are answered");
    let lines: Vec<Value> = String::from_utf8(out)
        .expect("utf-8")
        .lines()
        .map(|l| serde_json::from_str(l).expect("json line"))
        .collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[1]["result"]["isError"], false, "{lines:?}");
    assert!(!grant.config_path().exists(), "the config goes on connect");
}

#[test]
fn a_used_token_is_refused_with_the_gone_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let server = start(&tmp, "run");
    let grant = server.grant(caller(), &proxy()).expect("grant");
    let ping = format!("{}\n", json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}));
    for expect_ok in [true, false] {
        let mut out = Vec::new();
        run_proxy(
            server.socket_path(),
            grant.token(),
            std::io::Cursor::new(ping.clone().into_bytes()),
            &mut out,
        )
        .expect("proxy");
        let answer: Value = serde_json::from_slice(&out).expect("one json line");
        assert_eq!(answer.get("error").is_none(), expect_ok, "{answer}");
    }
}

#[test]
fn stale_runs_go_and_live_ones_stay() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut dead = std::process::Command::new("cmd")
        .args(["/C", "exit"])
        .spawn()
        .expect("spawn");
    let dead_pid = dead.id();
    dead.wait().expect("wait");
    drop(dead);
    for pid in [dead_pid, std::process::id()] {
        std::fs::create_dir_all(tmp.path().join(pid.to_string())).expect("mkdir");
    }
    remove_stale_runs(tmp.path());
    assert!(!tmp.path().join(dead_pid.to_string()).exists());
    assert!(tmp.path().join(std::process::id().to_string()).exists());
}
