//! The token line must arrive within one overall deadline: dripping a byte at a time can't hold a connection.
#![cfg(unix)]
use skyelab_agent::mcp::{Caller, Host, Proxy, Server, Tool};
use std::io::{Read as _, Write as _};
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
fn a_dripped_token_line_is_cut_at_the_overall_deadline() {
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
    let conn = UnixStream::connect(server.socket_path()).expect("connect");
    let at = Instant::now();
    let mut reader = conn.try_clone().expect("clone");
    let closed = std::thread::spawn(move || {
        let mut buf = [0u8; 16];
        let _ = reader.read(&mut buf); // returns once the server closes its side
        at.elapsed()
    });
    let mut writer = conn;
    // One byte of the real token every 700 ms, for 9 s: each read sees data well inside the 5 s timeout.
    for b in grant.token().bytes().take(13) {
        if closed.is_finished() || writer.write_all(&[b]).is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(700));
    }
    assert!(
        closed.is_finished(),
        "the server still holds the connection after 9 s"
    );
    let after = closed.join().expect("reader");
    assert!(
        after < Duration::from_millis(6_500),
        "closed after {after:?}, past the 5 s token deadline"
    );
}
