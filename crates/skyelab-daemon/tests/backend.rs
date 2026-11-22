//! The store-backed backend, end to end over the socket.

mod common;

use std::path::Path;

use common::*;
use skyelab_protocol::{Call, ErrorCode, Reply, ServerMessage};
use skyelab_store::Store;

fn seeded() -> (Store, i64, i64) {
    let mut store = Store::open_in_memory().expect("test setup");
    let p = store
        .create_project("app", Path::new("/src/app"))
        .expect("test setup");
    let t = store
        .create_thread(&skyelab_store::NewThread {
            project_id: p.id,
            title: "fix it",
            cwd: Path::new("/src/app"),
            account_id: "work",
            status: "idle",
        })
        .expect("test setup");
    for i in 1..=3 {
        store
            .append_event(t.id, "assistant", &format!(r#"{{"n":{i}}}"#))
            .expect("test setup");
    }
    (store, p.id, t.id)
}

#[test]
fn thread_events_since_returns_store_data() {
    let (store, _, thread_id) = seeded();
    let daemon = start(
        FakeTailnet::new(Peer::Own),
        store_backend(store),
        test_config(),
    );
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    send(&mut ws, 2, Call::ThreadEventsSince { thread_id, seq: 1 });
    match recv(&mut ws) {
        Some(ServerMessage::Response {
            id: 2,
            result: Reply::Events(events),
        }) => {
            assert_eq!(events.iter().map(|e| e.seq).collect::<Vec<_>>(), [2, 3]);
            assert_eq!(events[0].payload_json, r#"{"n":2}"#);
            assert!(
                events
                    .iter()
                    .all(|e| e.thread_id == thread_id && e.kind == "assistant")
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn lists_projects_and_threads() {
    let (store, project_id, thread_id) = seeded();
    let daemon = start(
        FakeTailnet::new(Peer::Own),
        store_backend(store),
        test_config(),
    );
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    send(&mut ws, 2, Call::ListProjects);
    match recv(&mut ws) {
        Some(ServerMessage::Response {
            result: Reply::Projects(p),
            ..
        }) => {
            assert_eq!(
                (p.len(), p[0].id, p[0].path.as_str()),
                (1, project_id, "/src/app")
            );
        }
        other => panic!("{other:?}"),
    }
    send(&mut ws, 3, Call::ListThreads { project_id });
    match recv(&mut ws) {
        Some(ServerMessage::Response {
            result: Reply::Threads(t),
            ..
        }) => {
            assert_eq!(
                (t.len(), t[0].id, t[0].account_id.as_str()),
                (1, thread_id, "work")
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn start_thread_not_wired() {
    let (store, project_id, thread_id) = seeded();
    let daemon = start(
        FakeTailnet::new(Peer::Own),
        store_backend(store),
        test_config(),
    );
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    send(
        &mut ws,
        2,
        Call::StartThread {
            project_id,
            prompt: "go".into(),
        },
    );
    assert_eq!(error_code(recv(&mut ws)), ErrorCode::NotWired);
    send(
        &mut ws,
        3,
        Call::SendMessage {
            thread_id,
            text: "more".into(),
        },
    );
    assert_eq!(error_code(recv(&mut ws)), ErrorCode::NotWired);
    send(&mut ws, 4, Call::Interrupt { thread_id });
    assert_eq!(error_code(recv(&mut ws)), ErrorCode::NotWired);
}
