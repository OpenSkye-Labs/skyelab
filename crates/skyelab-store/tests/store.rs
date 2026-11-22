#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use rusqlite::Connection;
use skyelab_store::Store;

fn store_with_project() -> (Store, i64) {
    let store = Store::open_in_memory().unwrap();
    let p = store
        .create_project("demo", Path::new("/work/demo"))
        .unwrap();
    (store, p.id)
}

/// Lets `updated_at` (ms precision) move forward between writes.
fn tick() {
    sleep(Duration::from_millis(5));
}

#[test]
fn migrates_empty_db() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    Store::open(&path).unwrap();

    let raw = Connection::open(&path).unwrap();
    let version: i64 = raw
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 1);
    let tables: Vec<String> = raw
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(tables, ["events", "projects", "threads"]);
    drop(raw);

    // Opening an already-migrated DB must not try to create the tables again.
    Store::open(&path).unwrap();
}

#[test]
fn refuses_db_from_newer_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    Connection::open(&path)
        .unwrap()
        .execute_batch("PRAGMA user_version = 99")
        .unwrap();
    assert!(Store::open(&path).is_err());
}

#[test]
fn open_file_sets_wal_and_fk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let _store = Store::open(&path).unwrap();

    let raw = Connection::open(&path).unwrap();
    let mode: String = raw
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");

    // foreign_keys is per connection, so check it through the store's behaviour.
    let store = Store::open(&path).unwrap();
    assert!(
        store
            .create_thread(12345, "t", Path::new("/x"), "acct", "idle")
            .is_err()
    );
}

#[test]
fn project_crud() {
    let store = Store::open_in_memory().unwrap();
    let a = store.create_project("a", Path::new("/work/a")).unwrap();
    let b = store.create_project("b", Path::new("/work/b")).unwrap();
    assert_eq!(a.name, "a");
    assert_eq!(a.path, Path::new("/work/a"));
    assert!(a.created_at > 0);

    assert_eq!(store.project(a.id).unwrap(), Some(a.clone()));
    assert_eq!(store.list_projects().unwrap(), vec![a.clone(), b.clone()]);

    // path is UNIQUE
    assert!(store.create_project("dup", Path::new("/work/a")).is_err());

    store.rename_project(a.id, "renamed").unwrap();
    assert_eq!(store.project(a.id).unwrap().unwrap().name, "renamed");

    store.delete_project(a.id).unwrap();
    assert_eq!(store.project(a.id).unwrap(), None);
    assert_eq!(store.list_projects().unwrap(), vec![b]);

    assert!(store.rename_project(a.id, "gone").is_err());
    assert!(store.delete_project(a.id).is_err());
}

#[test]
fn thread_crud() {
    let (store, pid) = store_with_project();
    let t = store
        .create_thread(pid, "first", Path::new("/work/demo"), "work", "idle")
        .unwrap();
    assert_eq!(t.project_id, pid);
    assert_eq!(t.title, "first");
    assert_eq!(t.cwd, Path::new("/work/demo"));
    assert_eq!(t.account_id, "work");
    assert_eq!(t.agent_session_id, None);
    assert_eq!(t.status, "idle");
    assert_eq!(t.created_at, t.updated_at);
    assert_eq!(store.thread(t.id).unwrap(), Some(t.clone()));

    tick();
    store.set_thread_title(t.id, "renamed").unwrap();
    store.set_thread_status(t.id, "running").unwrap();
    store.set_agent_session_id(t.id, Some("sess-1")).unwrap();
    let got = store.thread(t.id).unwrap().unwrap();
    assert_eq!(got.title, "renamed");
    assert_eq!(got.status, "running");
    assert_eq!(got.agent_session_id.as_deref(), Some("sess-1"));
    assert_eq!(got.account_id, "work");
    assert!(got.updated_at > t.updated_at);

    store.set_agent_session_id(t.id, None).unwrap();
    assert_eq!(store.thread(t.id).unwrap().unwrap().agent_session_id, None);

    store.delete_thread(t.id).unwrap();
    assert_eq!(store.thread(t.id).unwrap(), None);
    assert!(store.set_thread_status(t.id, "x").is_err());
    assert!(store.delete_thread(t.id).is_err());
}

#[test]
fn list_threads_orders_by_updated_desc() {
    let (mut store, pid) = store_with_project();
    let other = store.create_project("other", Path::new("/work/o")).unwrap();
    let cwd = Path::new("/work/demo");
    let t1 = store.create_thread(pid, "1", cwd, "a", "idle").unwrap();
    tick();
    let t2 = store.create_thread(pid, "2", cwd, "a", "idle").unwrap();
    tick();
    store
        .create_thread(other.id, "elsewhere", cwd, "a", "idle")
        .unwrap();

    let ids = |s: &Store| -> Vec<i64> {
        s.list_threads(pid)
            .unwrap()
            .into_iter()
            .map(|t| t.id)
            .collect()
    };
    assert_eq!(ids(&store), [t2.id, t1.id]);

    tick();
    store.append_event(t1.id, "msg", "{}").unwrap();
    assert_eq!(ids(&store), [t1.id, t2.id]);

    tick();
    store.set_thread_status(t2.id, "done").unwrap();
    assert_eq!(ids(&store), [t2.id, t1.id]);
}

#[test]
fn seq_monotonic_per_thread() {
    let (mut store, pid) = store_with_project();
    let cwd = Path::new("/work/demo");
    let a = store.create_thread(pid, "a", cwd, "x", "idle").unwrap();
    let b = store.create_thread(pid, "b", cwd, "x", "idle").unwrap();

    let seqs: Vec<i64> = (0..3)
        .map(|_| store.append_event(a.id, "k", "{}").unwrap().seq)
        .collect();
    assert_eq!(seqs, [1, 2, 3]);
    assert_eq!(store.append_event(b.id, "k", "{}").unwrap().seq, 1);
    assert_eq!(store.append_event(a.id, "k", "{}").unwrap().seq, 4);

    assert!(store.append_event(9999, "k", "{}").is_err());
}

#[test]
fn events_since_is_incremental() {
    let (mut store, pid) = store_with_project();
    let t = store
        .create_thread(pid, "t", Path::new("/work/demo"), "x", "idle")
        .unwrap();
    for i in 0..5 {
        store
            .append_event(t.id, "msg", &format!(r#"{{"i":{i}}}"#))
            .unwrap();
    }

    let all = store.events_since(t.id, 0).unwrap();
    assert_eq!(all.len(), 5);
    assert_eq!(all[0].thread_id, t.id);
    assert_eq!(all[0].kind, "msg");
    assert_eq!(all[0].payload_json, r#"{"i":0}"#);
    assert!(all[0].created_at > 0);

    let tail = store.events_since(t.id, 3).unwrap();
    assert_eq!(
        tail.iter().map(|e| e.seq).collect::<Vec<_>>(),
        [4, 5],
        "only events after the given seq, in order"
    );
    assert!(store.events_since(t.id, 5).unwrap().is_empty());
}

#[test]
fn project_delete_restricted_thread_delete_cascades() {
    let (mut store, pid) = store_with_project();
    let t = store
        .create_thread(pid, "t", Path::new("/work/demo"), "x", "idle")
        .unwrap();
    store.append_event(t.id, "msg", "{}").unwrap();

    // RESTRICT: a project with threads can't be deleted, and nothing is lost.
    assert!(store.delete_project(pid).is_err());
    assert!(store.project(pid).unwrap().is_some());
    assert!(store.thread(t.id).unwrap().is_some());

    // CASCADE: deleting a thread takes its events with it.
    store.delete_thread(t.id).unwrap();
    assert!(store.events_since(t.id, 0).unwrap().is_empty());
    store.delete_project(pid).unwrap();
}

#[test]
fn file_db_persists_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let (pid, tid) = {
        let mut store = Store::open(&path).unwrap();
        let p = store
            .create_project("demo", Path::new("/work/demo"))
            .unwrap();
        let t = store
            .create_thread(p.id, "t", Path::new("/work/demo"), "work", "idle")
            .unwrap();
        store.append_event(t.id, "msg", "{}").unwrap();
        (p.id, t.id)
    };

    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.project(pid).unwrap().unwrap().name, "demo");
    assert_eq!(store.thread(tid).unwrap().unwrap().account_id, "work");
    assert_eq!(store.events_since(tid, 0).unwrap().len(), 1);
    assert_eq!(store.append_event(tid, "msg", "{}").unwrap().seq, 2);
}
