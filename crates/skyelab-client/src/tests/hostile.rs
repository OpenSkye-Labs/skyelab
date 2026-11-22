//! A server that misbehaves mid-request closes the connection cleanly, and the request fails.

use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use skyelab_protocol::{Hello, Project, Reply, Request, ServerMessage, VERSION};
use tungstenite::{Message, WebSocket};

use super::*;
use crate::Error;

#[derive(Clone, Copy)]
enum Misbehave {
    Malformed,
    Oversized,
    Drop,
}

/// Every text frame each connection received, by connection.
type Log = Arc<Mutex<Vec<Vec<String>>>>;

/// Answers hello on every connection. On the first one it waits for one request, then misbehaves.
fn fake_server(how: Misbehave) -> (u16, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let log: Log = Arc::default();
    let server_log = log.clone();
    std::thread::spawn(move || {
        for (n, stream) in listener.incoming().enumerate() {
            let Ok(stream) = stream else { return };
            server_log.lock().expect("log").push(vec![]);
            let log = server_log.clone();
            std::thread::spawn(move || serve(stream, n, how, &log));
        }
    });
    (port, log)
}

fn read_text(ws: &mut WebSocket<TcpStream>) -> Option<String> {
    loop {
        match ws.read() {
            Ok(Message::Text(t)) => return Some(t.to_string()),
            Ok(_) => continue,
            Err(_) => return None,
        }
    }
}

fn serve(stream: TcpStream, n: usize, how: Misbehave, log: &Log) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    let Ok(mut ws) = tungstenite::accept(stream) else {
        return;
    };
    let record = |t: &str| log.lock().expect("log")[n].push(t.to_owned());
    let Some(hello) = read_text(&mut ws) else {
        return;
    };
    record(&hello);
    let id = serde_json::from_str::<Request>(&hello)
        .expect("hello request")
        .id;
    let reply = ServerMessage::Response {
        id,
        result: Reply::Hello(Hello {
            protocol: VERSION,
            capabilities: vec![],
        }),
    };
    ws.send(Message::text(reply.to_json())).expect("send hello");
    while let Some(text) = read_text(&mut ws) {
        record(&text);
        if n == 0 {
            let id = serde_json::from_str::<Request>(&text).expect("request").id;
            match how {
                Misbehave::Malformed => {
                    let _ = ws.send(Message::text("{\"type\": \"response\", \"id\": [nope"));
                }
                Misbehave::Oversized => {
                    // A valid answer, so only the client's size limit can reject it.
                    let big = ServerMessage::Response {
                        id,
                        result: Reply::Projects(vec![Project {
                            id: 1,
                            name: "x".repeat(64 * 1024),
                            path: "/p".into(),
                        }]),
                    };
                    let _ = ws.send(Message::text(big.to_json()));
                }
                Misbehave::Drop => return,
            }
            // Longer than the test waits, so only the client can be the one that closes.
            std::thread::sleep(Duration::from_secs(5));
            return;
        }
    }
}

fn check(how: Misbehave) {
    let (port, log) = fake_server(how);
    let opts = Options {
        max_message_bytes: 16 * 1024,
        ..options(port)
    };
    let client = Client::connect("127.0.0.1", opts).expect("client");
    connected(&client);
    let pending = client.list_projects();
    assert_eq!(
        pending.recv_timeout(Duration::from_secs(2)),
        Err(Error::ConnectionLost)
    );
    match next(&client) {
        Update::Disconnected(reason) => assert!(!reason.is_empty()),
        other => panic!("expected Disconnected, got {other:?}"),
    }
    connected(&client);
    std::thread::sleep(Duration::from_millis(300));
    let log = log.lock().expect("log");
    assert_eq!(
        log[0].len(),
        2,
        "hello and list_projects on the first connection"
    );
    assert!(log[0][1].contains("list_projects"), "{:?}", log[0]);
    assert_eq!(
        log[1].len(),
        1,
        "only hello on the second connection: {:?}",
        log[1]
    );
}

#[test]
fn malformed_frame_closes_cleanly() {
    check(Misbehave::Malformed);
}

#[test]
fn oversized_frame_closes_cleanly() {
    check(Misbehave::Oversized);
}

#[test]
fn dropped_connection_fails_the_request() {
    check(Misbehave::Drop);
}
