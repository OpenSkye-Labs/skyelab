//! A proxy whose connection to SkyeLab ends while the agent still talks (the app quit or restarted, or the grant was
//! revoked) answers every request itself with a clear error, instead of leaving the agent waiting.

use serde_json::{Value, json};
use skyelab_agent::mcp::{Caller, Host, Proxy, Server, Tool, run_proxy};
use std::io::{BufRead as _, BufReader, Write as _};
use std::sync::Arc;

struct Nop;
impl Host for Nop {
    fn call(&self, _: &Caller, _: Tool) -> Result<Value, String> {
        Ok(json!({"ok": true}))
    }
}

struct Agent {
    to_proxy: std::io::PipeWriter,
    from_proxy: BufReader<std::io::PipeReader>,
    proxy: std::thread::JoinHandle<anyhow::Result<()>>,
}

impl Agent {
    fn start(server: &Server, token: &str) -> Agent {
        let (input, to_proxy) = std::io::pipe().expect("pipe");
        let (from_proxy, output) = std::io::pipe().expect("pipe");
        let (socket, token) = (server.socket_path().to_owned(), token.to_owned());
        let proxy = std::thread::spawn(move || run_proxy(&socket, &token, input, output));
        Agent {
            to_proxy,
            from_proxy: BufReader::new(from_proxy),
            proxy,
        }
    }

    fn send(&mut self, v: Value) {
        writeln!(self.to_proxy, "{v}").expect("write to the proxy");
    }

    fn recv(&mut self) -> Value {
        let mut line = String::new();
        self.from_proxy.read_line(&mut line).expect("read");
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line:?}"))
    }

    /// Ends the agent's input; the proxy must then return.
    fn finish(self) {
        drop(self.to_proxy);
        self.proxy
            .join()
            .expect("proxy thread")
            .expect("the proxy returns Ok once its input ends");
    }
}

fn call(id: i64) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": "list_projects", "arguments": {}}})
}

fn rig() -> (tempfile::TempDir, Server) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let server = Server::start(&tmp.path().join("run"), Arc::new(Nop)).expect("start");
    (tmp, server)
}

fn caller() -> Caller {
    Caller {
        thread_id: 1,
        account_id: "acme".into(),
    }
}

fn proxy() -> Proxy {
    Proxy {
        program: "/bin/true".into(),
        args: vec![],
    }
}

fn assert_gone_text(text: &str) {
    assert!(
        text.contains("SkyeLab") && text.contains("start the thread again"),
        "says what happened and what to do: {text}"
    );
}

#[test]
fn revoked_mid_session_answers_with_a_clear_error() {
    let (_tmp, server) = rig();
    let grant = server.grant(caller(), &proxy()).expect("grant");
    let mut agent = Agent::start(&server, grant.token());
    agent.send(call(1));
    let ok = agent.recv();
    assert_eq!(ok["id"], 1);
    assert_eq!(ok["result"]["isError"], false, "{ok}");

    drop(grant); // the server cuts the connection
    agent.send(call(2));
    let gone = agent.recv();
    assert_eq!(gone["id"], 2);
    assert_eq!(
        gone["result"]["isError"], true,
        "a tool error the agent reads: {gone}"
    );
    assert_gone_text(gone["result"]["content"][0]["text"].as_str().expect("text"));

    agent.send(json!({"jsonrpc": "2.0", "method": "notifications/cancelled"}));
    agent.send(json!({"jsonrpc": "2.0", "id": "p", "method": "ping"}));
    let ping = agent.recv();
    assert_eq!(ping["id"], "p", "notifications get no answer: {ping}");
    assert_eq!(ping["error"]["code"], -32000);
    assert_gone_text(ping["error"]["message"].as_str().expect("message"));
    agent.finish();
}

#[test]
fn refused_token_answers_initialize_with_an_error() {
    let (_tmp, server) = rig();
    let mut agent = Agent::start(&server, &"0".repeat(64));
    agent.send(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}));
    let answer = agent.recv();
    assert_eq!(answer["id"], 1);
    assert_eq!(answer["error"]["code"], -32000, "{answer}");
    assert_gone_text(answer["error"]["message"].as_str().expect("message"));
    agent.finish();
}

#[test]
fn a_last_line_without_a_newline_is_answered() {
    let (_tmp, server) = rig();
    let grant = server.grant(caller(), &proxy()).expect("grant");
    let (input, mut to_proxy) = std::io::pipe().expect("pipe");
    let (mut from_proxy, output) = std::io::pipe().expect("pipe");
    let (socket, token) = (server.socket_path().to_owned(), grant.token().to_owned());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(run_proxy(&socket, &token, input, output).is_ok());
    });
    write!(to_proxy, r#"{{"jsonrpc":"2.0","id":1,"method":"ping"}}"#).expect("write");
    drop(to_proxy);
    let done = rx.recv_timeout(std::time::Duration::from_secs(5));
    assert!(matches!(done, Ok(true)), "the proxy hung: {done:?}");
    let mut out = String::new();
    std::io::Read::read_to_string(&mut from_proxy, &mut out).expect("read");
    assert!(out.contains(r#""id":1"#), "{out}");
}
