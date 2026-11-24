//! Dropping a grant closes its idle connection at once, not on the agent's next line.
#![cfg(unix)]
use skyelab_agent::mcp::{Caller, Host, Proxy, Server, Tool};
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Nop;
impl Host for Nop {
    fn call(&self, _: &Caller, _: Tool) -> Result<serde_json::Value, String> {
        Ok(serde_json::Value::Null)
    }
}

#[test]
fn revoking_cuts_an_idle_connection() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let server = Server::start(&tmp.path().join("run"), Arc::new(Nop)).expect("start");
    let caller = Caller {
        thread_id: 1,
        account_id: "acme".into(),
    };
    let proxy = Proxy {
        program: "/bin/true".into(),
        args: vec![],
    };
    let grant = server.grant(caller, &proxy).expect("grant");
    let mut conn = UnixStream::connect(server.socket_path()).expect("connect");
    conn.set_read_timeout(Some(Duration::from_secs(60)))
        .expect("timeout");
    writeln!(conn, "{}", grant.token()).expect("token");
    writeln!(conn, r#"{{"jsonrpc":"2.0","id":1,"method":"ping"}}"#).expect("ping");
    let mut reader = BufReader::new(conn.try_clone().expect("clone"));
    let mut line = String::new();
    reader.read_line(&mut line).expect("pong");
    assert!(line.contains(r#""id":1"#), "{line}");

    let at = Instant::now();
    drop(grant);
    line.clear();
    let n = reader.read_line(&mut line).expect("EOF, not a timeout");
    assert_eq!(n, 0, "the connection closes: {line}");
    assert!(at.elapsed() < Duration::from_secs(5), "closed at once");
}
