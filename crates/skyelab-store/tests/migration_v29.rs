//! Schema v29 adds delegation across machines (`delegated_from`, `remote_children`) and a random store id, without
//! touching existing rows.

use rusqlite::Connection;
use skyelab_store::{DelegatedOrigin, NewThread, RemoteChild, Store};

#[test]
fn a_v28_database_upgrades_and_keeps_its_rows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("store.db");
    let mut store = Store::open(&path).expect("open");
    let p = store
        .create_project("demo", std::path::Path::new("/work/demo"))
        .expect("project");
    let t = store
        .create_thread(&NewThread {
            project_id: p.id,
            title: "kept",
            cwd: &p.path,
            account_id: "acme",
            status: "idle",
        })
        .expect("thread");
    store.append_event(t.id, "prompt", "\"hi\"").expect("event");
    drop(store);
    let conn = Connection::open(&path).expect("raw");
    conn.execute_batch(
        "DROP TABLE delegated_from; DROP TABLE remote_children; DROP TABLE store_meta; PRAGMA user_version = 28;",
    )
    .expect("back to v28");
    drop(conn);

    let store = Store::open(&path).expect("migrates");
    let kept = store.thread(t.id).expect("read").expect("kept");
    assert_eq!(
        (kept.title.as_str(), kept.status.as_str()),
        ("kept", "idle")
    );
    assert_eq!(store.events_since(t.id, 0).expect("events").len(), 1);
    let id = store.store_id().expect("store id");
    assert!(
        id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "{id}"
    );
    store
        .set_delegated_from(
            t.id,
            &DelegatedOrigin {
                origin_node: "nA".into(),
                origin_store: "s".into(),
                origin_machine: "a.example.ts.net".into(),
                parent_thread_id: 3,
                parent_title: "p".into(),
                origin_account: "x".into(),
                depth: 1,
                request_id: Some("r".into()),
            },
        )
        .expect("delegated_from exists");
    store
        .add_remote_child(&RemoteChild {
            parent_thread_id: t.id,
            machine: "b.example.ts.net".into(),
            node_id: "nB".into(),
            thread_id: 9,
            title: "child".into(),
            created_at: 0,
        })
        .expect("remote_children exists");
    drop(store);
    let store = Store::open(&path).expect("reopen");
    assert_eq!(store.store_id().expect("id"), id, "made once");
    assert_eq!(store.remote_children(t.id).expect("read").len(), 1);
    let conn = Connection::open(&path).expect("raw");
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("version");
    assert_eq!(version, 29);
}

#[test]
fn a_fresh_database_reaches_v29_with_its_own_store_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (a, b) = (dir.path().join("a.db"), dir.path().join("b.db"));
    let ids: Vec<String> = [&a, &b]
        .iter()
        .map(|p| Store::open(p).expect("open").store_id().expect("id"))
        .collect();
    assert_ne!(ids[0], ids[1], "each store its own id");
    let version: i64 = Connection::open(&a)
        .expect("raw")
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("version");
    assert_eq!(version, 29);
}
