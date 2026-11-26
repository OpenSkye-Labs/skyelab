//! A shell subscribe (1.7) or thread listing (< 1.7) that fails while connected is retried on a timer, not left
//! empty until the next reconnect.

pub mod common;
#[path = "common/tls_ws.rs"]
mod tls_ws;

use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use common::{NODE_NAME, options};
use skyelab_mobile::{
    remote::RemoteSource,
    source::{Connection, ThreadSource},
};
use skyelab_protocol::{
    Call, ErrorCode, Hello, Project, Reply, Request, RpcError, ServerMessage, ShellSnapshot,
    Thread, Version,
};
use tungstenite::Message;

fn sample_project() -> Project {
    Project {
        id: 1,
        name: "app".into(),
        path: "/src/app".into(),
        created_at: 0,
    }
}

fn sample_thread() -> Thread {
    Thread {
        id: 10,
        project_id: 1,
        title: "fix it".into(),
        account_id: "work".into(),
        account_label: Some("Work".into()),
        status: "idle".into(),
        pinned: false,
        preview: None,
        settled_at: None,
        snoozed_until: None,
        created_at: 100,
        updated_at: 100,
        row: Default::default(),
    }
}

/// Starts a mock plaintext WS server speaking protocol 1.`minor` that fails its first thread/shell fetch once.
fn flaky_server(minor: u32) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let fails_left = AtomicUsize::new(1);
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let Some(mut ws) = tls_ws::accept(stream) else {
            return;
        };
        while let Ok(msg) = ws.read() {
            let Message::Text(text) = msg else { continue };
            let req: Request = serde_json::from_str(&text).expect("request");
            let reply = match req.call {
                Call::Hello { .. } => ServerMessage::Response {
                    id: req.id,
                    result: Reply::Hello(Hello {
                        protocol: Version { major: 1, minor },
                        capabilities: vec![],
                        node_id: None,
                    }),
                },
                Call::SubscribeShell => {
                    if fails_left.fetch_sub(1, Ordering::SeqCst) > 0 {
                        ServerMessage::Error {
                            id: Some(req.id),
                            error: RpcError::new(ErrorCode::Internal, "store busy"),
                        }
                    } else {
                        ServerMessage::Response {
                            id: req.id,
                            result: Reply::Shell(ShellSnapshot {
                                seq: 1,
                                projects: vec![sample_project()],
                                threads: vec![sample_thread()],
                                truncated: false,
                            }),
                        }
                    }
                }
                Call::ListProjects => ServerMessage::Response {
                    id: req.id,
                    result: Reply::Projects(vec![sample_project()]),
                },
                Call::ListThreads { .. } => {
                    if fails_left.fetch_sub(1, Ordering::SeqCst) > 0 {
                        ServerMessage::Error {
                            id: Some(req.id),
                            error: RpcError::new(ErrorCode::Internal, "store busy"),
                        }
                    } else {
                        ServerMessage::Response {
                            id: req.id,
                            result: Reply::Threads(vec![sample_thread()]),
                        }
                    }
                }
                _ => ServerMessage::Response {
                    id: req.id,
                    result: Reply::Ok,
                },
            };
            if ws.send(Message::text(reply.to_json())).is_err() {
                break;
            }
        }
    });
    port
}

fn verify_retry(minor: u32) {
    let port = flaky_server(minor);
    let pins = tempfile::tempdir().expect("pin dir");
    let mut source = RemoteSource::connect(NODE_NAME, options(port), pins.path()).expect("client");
    let inbox = source.inbox().expect("inbox");
    let deadline = Instant::now() + Duration::from_secs(10);
    let settle = |source: &mut RemoteSource, what: &str, done: &dyn Fn(&RemoteSource) -> bool| {
        while !done(source) {
            let left = deadline.saturating_duration_since(Instant::now());
            let msg = inbox
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
            source.apply(msg);
        }
    };
    settle(&mut source, "the error", &|s| {
        s.connection() == Connection::Connected && s.error().is_some()
    });
    assert!(source.threads().is_empty());
    settle(&mut source, "the retried list", &|s| {
        !s.threads().is_empty()
    });
    assert_eq!(source.error(), None);
}

#[test]
fn a_failed_shell_subscribe_is_retried() {
    verify_retry(7);
}

#[test]
fn a_failed_legacy_thread_list_is_retried() {
    verify_retry(6);
}
