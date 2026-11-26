//! `subscribe_shell` (1.7): a snapshot, then every row write as a numbered change, only to approved connections
//! that asked for it. A 1.6 client never sees a shell message.

mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};

use common::*;
use skyelab_daemon::testing::Peer;
use skyelab_daemon::{Backend, NotWired, StoreBackend};
use skyelab_protocol::{Call, ErrorCode, Reply, ServerMessage, ShellDelta, Version};
use skyelab_store::{NewThread, Store};

fn served() -> (Arc<Mutex<Store>>, skyelab_daemon::Daemon, i64, i64) {
    let store = Store::open_in_memory().expect("store");
    let p = store
        .create_project("app", Path::new("/src/app"))
        .expect("project");
    let t = store
        .create_thread(&NewThread {
            project_id: p.id,
            title: "fix it",
            cwd: Path::new("/src/app"),
            account_id: "work",
            status: "idle",
        })
        .expect("thread");
    let store = Arc::new(Mutex::new(store));
    let backend: Arc<dyn Backend> = Arc::new(StoreBackend::shared(store.clone(), NotWired, None));
    let daemon = start(FakeTailnet::new(Peer::Own), backend, test_config());
    (store, daemon, p.id, t.id)
}

fn subscribe(ws: &mut Client) -> skyelab_protocol::ShellSnapshot {
    send(ws, 2, Call::SubscribeShell);
    match recv(ws) {
        Some(ServerMessage::Response {
            id: 2,
            result: Reply::Shell(s),
        }) => s,
        other => panic!("expected the snapshot, got {other:?}"),
    }
}

fn change(ws: &mut Client) -> (u64, ShellDelta) {
    match recv(ws) {
        Some(ServerMessage::Shell(c)) => (c.seq, c.delta),
        other => panic!("expected a shell change, got {other:?}"),
    }
}

#[test]
fn a_snapshot_then_each_write_in_order() {
    let (store, daemon, project, thread) = served();
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    let snap = subscribe(&mut ws);
    assert_eq!(snap.projects.len(), 1);
    assert_eq!(snap.threads[0].title, "fix it");
    assert_eq!(
        snap.threads[0].row.cwd, "/src/app",
        "1.7 row fields ride along"
    );

    store
        .lock()
        .expect("lock")
        .set_thread_title(thread, "renamed")
        .expect("rename");
    let (seq, delta) = change(&mut ws);
    assert_eq!(seq, snap.seq + 1);
    match delta {
        ShellDelta::ThreadUpdated(t) => assert_eq!(t.title, "renamed"),
        other => panic!("{other:?}"),
    }

    store
        .lock()
        .expect("lock")
        .set_thread_settled(thread, true)
        .expect("settle");
    match change(&mut ws) {
        (s, ShellDelta::ThreadUpdated(t)) => {
            assert_eq!(s, seq + 1, "no gap");
            assert!(t.settled_at.is_some());
        }
        other => panic!("{other:?}"),
    }

    store
        .lock()
        .expect("lock")
        .set_project_removed(project, true)
        .expect("remove");
    assert_eq!(
        change(&mut ws).1,
        ShellDelta::ProjectRemoved { id: project }
    );
}

#[test]
fn an_unapproved_device_gets_no_snapshot() {
    let (_store, daemon, _, _) = served();
    daemon.set_approved(vec![]);
    let mut ws = connect(daemon.local_addrs()[0]);
    send(&mut ws, 5, Call::SubscribeShell);
    assert_eq!(error_code(recv(&mut ws)), ErrorCode::HelloRequired);
    let mut ws = connect(daemon.local_addrs()[0]);
    assert_eq!(
        error_code(hello_key(&mut ws, Some(KEY))),
        ErrorCode::NotApproved
    );
    assert_eq!(recv(&mut ws), None, "closed, nothing streamed");
}

#[test]
fn a_revoked_device_stops_streaming() {
    let (store, daemon, _, thread) = served();
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    subscribe(&mut ws);
    daemon.set_approved(vec![]);
    store
        .lock()
        .expect("lock")
        .set_thread_title(thread, "after")
        .expect("rename");
    assert_eq!(recv(&mut ws), None, "closed before any change went out");
}

#[test]
fn a_1_6_client_gets_no_shell_messages() {
    let (store, daemon, _, thread) = served();
    let addr = daemon.local_addrs()[0];
    let mut old = connect(addr);
    assert!(matches!(
        hello_with(&mut old, Version { major: 1, minor: 6 }),
        Some(ServerMessage::Response { .. })
    ));
    let mut new = connect_hello(addr);
    subscribe(&mut new);
    store
        .lock()
        .expect("lock")
        .set_thread_title(thread, "renamed")
        .expect("rename");
    change(&mut new); // the stream is live
    send(&mut old, 9, Call::ListProjects);
    assert!(
        matches!(recv(&mut old), Some(ServerMessage::Response { id: 9, .. })),
        "the old client's next message is its own reply, no shell push"
    );
}

/// One write that fans out past the backlog (renaming a project sends each of its threads) doesn't drop a
/// subscriber that keeps up: the backlog is checked per batch.
#[test]
fn a_big_fan_out_keeps_the_stream() {
    let store = Store::open_in_memory().expect("store");
    let p = store
        .create_project("app", Path::new("/src/app"))
        .expect("project");
    for i in 0..1100 {
        let title = format!("t{i}");
        store
            .create_thread(&NewThread {
                project_id: p.id,
                title: &title,
                cwd: Path::new("/src/app"),
                account_id: "work",
                status: "idle",
            })
            .expect("thread");
    }
    let store = Arc::new(Mutex::new(store));
    let backend = StoreBackend::shared(store.clone(), NotWired, None);
    let feed = backend.subscribe_shell().expect("snapshot");
    store
        .lock()
        .expect("lock")
        .rename_project(p.id, "renamed")
        .expect("rename");
    std::thread::sleep(std::time::Duration::from_millis(500)); // the whole batch is queued before we read
    let wait = std::time::Duration::from_secs(10);
    let got = std::iter::from_fn(|| feed.changes.recv_timeout(wait).ok())
        .take(1101)
        .count();
    assert_eq!(got, 1101, "the project and each of its threads");
}

/// A store whose snapshot would pass the client's message limit sends the newest threads that fit, marked
/// truncated, archived ones left out first; later changes still stream.
#[test]
fn a_snapshot_too_big_for_one_message_is_cut_to_the_newest() {
    let (store, daemon, project, _) = served();
    let title = "x".repeat(4000);
    let mut last = 0;
    {
        let store = store.lock().expect("lock");
        let archived = store
            .create_thread(&NewThread {
                project_id: project,
                title: "archived but new",
                cwd: Path::new("/src/app"),
                account_id: "work",
                status: "idle",
            })
            .expect("thread")
            .id;
        store.set_thread_archived(archived, true).expect("archive");
        for _ in 0..2500 {
            last = store
                .create_thread(&NewThread {
                    project_id: project,
                    title: &title,
                    cwd: Path::new("/src/app"),
                    account_id: "work",
                    status: "idle",
                })
                .expect("thread")
                .id;
        }
    }
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    send(&mut ws, 2, Call::SubscribeShell);
    let reply = recv(&mut ws).expect("a reply");
    assert!(
        reply.to_json().len() <= skyelab_daemon::MAX_REPLY_BYTES,
        "{} bytes",
        reply.to_json().len()
    );
    let ServerMessage::Response {
        result: Reply::Shell(snap),
        ..
    } = reply
    else {
        panic!("expected the snapshot");
    };
    assert!(snap.truncated);
    assert!(snap.threads.len() > 1000 && snap.threads.len() < 2502);
    assert!(
        snap.threads.iter().any(|t| t.id == last),
        "the newest is kept"
    );
    assert!(
        !snap.threads.iter().any(|t| t.row.archived_at.is_some()),
        "archived ones go first"
    );
    store
        .lock()
        .expect("lock")
        .set_thread_title(last, "still streams")
        .expect("rename");
    assert!(
        matches!(change(&mut ws).1, ShellDelta::ThreadUpdated(t) if t.title == "still streams")
    );
}
