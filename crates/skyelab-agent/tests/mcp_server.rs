//! SkyeLab's MCP server against a fake host: the protocol, every tool, input validation, tokens and the proxy.
#![cfg(unix)]

use serde_json::{Value, json};
use skyelab_agent::mcp::{Caller, Grant, Host, Proxy, Server, Tool, run_proxy};
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
struct FakeHost {
    calls: Mutex<Vec<(i64, String, Tool)>>,
}

impl Host for FakeHost {
    fn call(&self, caller: &Caller, tool: Tool) -> Result<Value, String> {
        let answer = match &tool {
            Tool::ListProjects => Ok(json!([{"id": 1, "name": "demo", "path": "/work/demo"}])),
            Tool::StartThread { project_id: 99, .. } => Err("refused: other account".to_owned()),
            Tool::StartThread { .. } => Ok(json!({"thread_id": 42})),
            _ => Ok(json!({"ok": true})),
        };
        self.calls
            .lock()
            .expect("lock")
            .push((caller.thread_id, caller.account_id.clone(), tool));
        answer
    }
}

struct Rig {
    _tmp: tempfile::TempDir,
    host: Arc<FakeHost>,
    server: Server,
}

fn rig() -> Rig {
    let tmp = tempfile::tempdir().expect("tempdir");
    let host = Arc::new(FakeHost::default());
    let server = Server::start(&tmp.path().join("run"), host.clone()).expect("server starts");
    Rig {
        _tmp: tmp,
        host,
        server,
    }
}

fn proxy() -> Proxy {
    Proxy {
        program: PathBuf::from("/usr/local/bin/skyelab"),
        args: vec!["--mcp-proxy".into()],
    }
}

fn grant(r: &Rig, thread_id: i64) -> Grant {
    r.server
        .grant(
            Caller {
                thread_id,
                account_id: "acme".into(),
            },
            &proxy(),
        )
        .expect("grant")
}

struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next: i64,
}

impl Client {
    fn connect(r: &Rig, token: &str) -> Client {
        let mut writer = UnixStream::connect(r.server.socket_path()).expect("connect");
        writer
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("timeout");
        writeln!(writer, "{token}").expect("token");
        let reader = BufReader::new(writer.try_clone().expect("clone"));
        Client {
            reader,
            writer,
            next: 0,
        }
    }

    /// A refused connection may be closed already; the answer (none) is what tests check.
    fn send(&mut self, v: &Value) {
        let _ = writeln!(self.writer, "{v}");
    }

    fn recv(&mut self) -> Option<Value> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(serde_json::from_str(&line).expect("answer is JSON")),
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let answer = self.recv().expect("an answer");
        assert_eq!(answer["jsonrpc"], "2.0");
        assert_eq!(answer["id"], id);
        answer
    }

    fn tool(&mut self, name: &str, args: Value) -> (bool, String) {
        let a = self.request("tools/call", json!({"name": name, "arguments": args}));
        let result = &a["result"];
        let text = result["content"][0]["text"]
            .as_str()
            .expect("text content")
            .to_owned();
        assert_eq!(result["content"][0]["type"], "text");
        (result["isError"].as_bool().unwrap_or(false), text)
    }
}

fn initialized(r: &Rig, g: &Grant) -> Client {
    let mut c = Client::connect(r, g.token());
    let init = c.request(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}),
    );
    assert!(init["result"]["protocolVersion"].is_string(), "{init}");
    assert!(
        init["result"]["capabilities"]["tools"].is_object(),
        "{init}"
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "skyelab");
    c.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    c
}

#[test]
fn lists_all_six_tools_with_schemas() {
    let r = rig();
    let g = grant(&r, 7);
    let mut c = initialized(&r, &g);
    let list = c.request("tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().expect("tools");
    let mut names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    names.sort();
    assert_eq!(
        names,
        [
            "list_projects",
            "read_thread",
            "send_message",
            "start_thread",
            "thread_status",
            "wait_thread"
        ]
    );
    for t in tools {
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        assert!(t["description"].is_string(), "{t}");
    }
    let ping = c.request("ping", json!({}));
    assert!(ping["result"].is_object(), "{ping}");
    let unknown = c.request("resources/list", json!({}));
    assert_eq!(unknown["error"]["code"], -32601, "{unknown}");
}

#[test]
fn each_tool_reaches_the_host_with_the_caller() {
    let r = rig();
    let g = grant(&r, 7);
    let mut c = initialized(&r, &g);
    assert!(!c.tool("list_projects", json!({})).0);
    let (err, text) = c.tool(
        "start_thread",
        json!({"project_id": 3, "prompt": "fix it", "worktree": true}),
    );
    assert!(!err, "{text}");
    assert_eq!(
        serde_json::from_str::<Value>(&text).expect("json")["thread_id"],
        42
    );
    c.tool("start_thread", json!({"project_id": 4, "prompt": "plain"}));
    c.tool("thread_status", json!({"thread_id": 5}));
    c.tool("read_thread", json!({"thread_id": 5, "since_seq": 3}));
    c.tool("read_thread", json!({"thread_id": 5}));
    c.tool("send_message", json!({"thread_id": 5, "text": "more"}));
    c.tool("wait_thread", json!({"thread_id": 5, "timeout_s": 30}));
    let calls = r.host.calls.lock().expect("lock");
    let tools: Vec<&Tool> = calls.iter().map(|(_, _, t)| t).collect();
    assert_eq!(
        tools,
        [
            &Tool::ListProjects,
            &Tool::StartThread {
                project_id: 3,
                prompt: "fix it".into(),
                worktree: true
            },
            &Tool::StartThread {
                project_id: 4,
                prompt: "plain".into(),
                worktree: false
            },
            &Tool::ThreadStatus { thread_id: 5 },
            &Tool::ReadThread {
                thread_id: 5,
                since_seq: 3
            },
            &Tool::ReadThread {
                thread_id: 5,
                since_seq: 0
            },
            &Tool::SendMessage {
                thread_id: 5,
                text: "more".into()
            },
            &Tool::WaitThread {
                thread_id: 5,
                timeout_s: 30
            },
        ]
    );
    assert!(calls.iter().all(|(id, acct, _)| *id == 7 && acct == "acme"));
}

#[test]
fn a_host_refusal_is_a_tool_error() {
    let r = rig();
    let g = grant(&r, 7);
    let mut c = initialized(&r, &g);
    let (err, text) = c.tool("start_thread", json!({"project_id": 99, "prompt": "x"}));
    assert!(err);
    assert!(text.contains("other account"), "{text}");
}

#[test]
fn bad_inputs_are_refused_before_the_host() {
    let r = rig();
    let g = grant(&r, 7);
    let mut c = initialized(&r, &g);
    let long = "x".repeat(100_001);
    for (name, args, field) in [
        ("start_thread", json!({"prompt": "x"}), "project_id"),
        (
            "start_thread",
            json!({"project_id": "3", "prompt": "x"}),
            "project_id",
        ),
        (
            "start_thread",
            json!({"project_id": 0, "prompt": "x"}),
            "project_id",
        ),
        (
            "start_thread",
            json!({"project_id": -2, "prompt": "x"}),
            "project_id",
        ),
        (
            "start_thread",
            json!({"project_id": 3, "prompt": ""}),
            "prompt",
        ),
        (
            "start_thread",
            json!({"project_id": 3, "prompt": "   "}),
            "prompt",
        ),
        (
            "start_thread",
            json!({"project_id": 3, "prompt": long}),
            "prompt",
        ),
        (
            "start_thread",
            json!({"project_id": 3, "prompt": "x", "worktree": "yes"}),
            "worktree",
        ),
        ("thread_status", json!({"thread_id": 1.5}), "thread_id"),
        (
            "read_thread",
            json!({"thread_id": 1, "since_seq": -1}),
            "since_seq",
        ),
        ("send_message", json!({"thread_id": 1}), "text"),
        (
            "wait_thread",
            json!({"thread_id": 1, "timeout_s": 0}),
            "timeout_s",
        ),
        (
            "wait_thread",
            json!({"thread_id": 1, "timeout_s": 301}),
            "timeout_s",
        ),
        ("wait_thread", json!({"thread_id": 1}), "timeout_s"),
        ("no_such_tool", json!({}), "no_such_tool"),
    ] {
        let (err, text) = c.tool(name, args.clone());
        assert!(err, "{name} {args} should be refused");
        assert!(text.contains(field), "{name} {args}: {text}");
    }
    // Arguments that aren't an object at all.
    let a = c.request(
        "tools/call",
        json!({"name": "thread_status", "arguments": [1]}),
    );
    assert_eq!(a["result"]["isError"], true, "{a}");
    assert!(r.host.calls.lock().expect("lock").is_empty());
}

#[test]
fn mcp_bad_token_gets_no_answer() {
    let r = rig();
    let g = grant(&r, 7);
    for token in ["nope", "", &"0".repeat(64)] {
        let mut c = Client::connect(&r, token);
        c.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}));
        assert_eq!(c.recv(), None, "token {token:?} must get no answer");
    }
    // A dropped grant's token stops working, and so does its config file.
    let token = g.token().to_owned();
    let config = g.config_path().to_owned();
    assert!(config.exists());
    drop(g);
    assert!(!config.exists(), "the config file goes with the grant");
    let mut c = Client::connect(&r, &token);
    c.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}));
    assert_eq!(c.recv(), None);
    assert!(r.host.calls.lock().expect("lock").is_empty());
}

/// A token works for one connection, and its config file is gone once claude's proxy connected, so another
/// agent of the same OS user can't read it and act as this caller.
#[test]
fn a_token_is_single_use_and_its_file_goes_on_connect() {
    let r = rig();
    let g = grant(&r, 7);
    assert!(g.config_path().exists());
    let mut first = initialized(&r, &g);
    assert!(
        !g.config_path().exists(),
        "deleted once the session connected"
    );
    let mut second = Client::connect(&r, g.token());
    second.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}));
    assert_eq!(
        second.recv(),
        None,
        "a second connection with the same token is refused"
    );
    assert!(
        !first.tool("list_projects", json!({})).0,
        "the first keeps working"
    );
}

/// Dropping the grant (the agent ended) cuts an already open connection too.
#[test]
fn an_open_connection_stops_when_its_grant_drops() {
    let r = rig();
    let g = grant(&r, 7);
    let mut c = initialized(&r, &g);
    drop(g);
    // The server may already have closed (on the notification after the drop), so the write may fail.
    let call = json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "list_projects", "arguments": {}}});
    let _ = writeln!(c.writer, "{call}");
    assert_eq!(c.recv(), None);
    assert!(r.host.calls.lock().expect("lock").is_empty());
}

#[test]
fn grant_writes_a_private_config_naming_the_proxy() {
    use std::os::unix::fs::PermissionsExt as _;
    let r = rig();
    let g = grant(&r, 7);
    let other = grant(&r, 8);
    assert_ne!(g.token(), other.token(), "tokens are per grant");
    assert!(g.token().len() >= 32);
    let mode =
        |p: &std::path::Path| std::fs::metadata(p).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode(g.config_path()), 0o600);
    assert_eq!(mode(r.server.socket_path().parent().expect("dir")), 0o700);
    let config: Value =
        serde_json::from_str(&std::fs::read_to_string(g.config_path()).expect("read"))
            .expect("json");
    let s = &config["mcpServers"]["skyelab"];
    assert_eq!(
        s["type"], "stdio",
        "Claude Code's documented mcpServers format"
    );
    assert_eq!(s["command"], "/usr/local/bin/skyelab");
    assert_eq!(s["args"], json!(["--mcp-proxy"]));
    assert_eq!(s["env"]["SKYELAB_MCP_TOKEN"], g.token());
    assert_eq!(
        s["env"]["SKYELAB_MCP_SOCKET"],
        r.server.socket_path().to_str().expect("utf-8")
    );
}

#[test]
fn mcp_proxy_sends_the_token_then_pipes_both_ways() {
    let r = rig();
    let g = grant(&r, 7);
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
        r.server.socket_path(),
        g.token(),
        std::io::Cursor::new(input.into_bytes()),
        &mut out,
    )
    .expect("proxy runs until the server closes");
    let lines: Vec<Value> = String::from_utf8(out)
        .expect("utf-8")
        .lines()
        .map(|l| serde_json::from_str(l).expect("json line"))
        .collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0]["id"], 1);
    assert_eq!(lines[1]["id"], 2);
    assert_eq!(r.host.calls.lock().expect("lock").len(), 1);
}

#[test]
fn an_oversized_line_closes_the_connection() {
    let r = rig();
    let g = grant(&r, 7);
    let mut c = initialized(&r, &g);
    let big = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\",\"pad\":\"{}\"}}",
        "a".repeat(2 << 20)
    );
    let _ = writeln!(c.writer, "{big}");
    assert_eq!(c.recv(), None);
}
