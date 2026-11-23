use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use rusqlite::Connection;
use skyelab_store::{NewThread, Store};

fn store_with_project() -> (Store, i64) {
    let store = Store::open_in_memory().expect("test setup");
    let p = store
        .create_project("demo", Path::new("/work/demo"))
        .expect("test setup");
    (store, p.id)
}

/// Lets `updated_at` (ms precision) move forward between writes.
fn tick() {
    sleep(Duration::from_millis(5));
}

#[test]
fn migrates_empty_db() {
    let dir = tempfile::tempdir().expect("test setup");
    let path = dir.path().join("store.db");
    Store::open(&path).expect("test setup");

    let raw = Connection::open(&path).expect("test setup");
    let version: i64 = raw
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("test setup");
    assert_eq!(version, 14);
    let tables: Vec<String> = raw
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'event_text%' ORDER BY name")
        .expect("test setup")
        .query_map([], |r| r.get(0))
        .expect("test setup")
        .collect::<Result<_, _>>()
        .expect("test setup");
    assert_eq!(
        tables,
        [
            "draft_files",
            "drafts",
            "events",
            "project_scripts",
            "projects",
            "threads",
            "usage_readings"
        ]
    );
    drop(raw);

    // Opening an already-migrated DB must not try to create the tables again.
    Store::open(&path).expect("test setup");
}

#[test]
fn refuses_db_from_newer_version() {
    let dir = tempfile::tempdir().expect("test setup");
    let path = dir.path().join("store.db");
    Connection::open(&path)
        .expect("test setup")
        .execute_batch("PRAGMA user_version = 99")
        .expect("test setup");
    assert!(Store::open(&path).is_err());
}

#[test]
fn open_file_sets_wal_and_fk() {
    let dir = tempfile::tempdir().expect("test setup");
    let path = dir.path().join("store.db");
    let _store = Store::open(&path).expect("test setup");

    let raw = Connection::open(&path).expect("test setup");
    let mode: String = raw
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .expect("test setup");
    assert_eq!(mode, "wal");

    // foreign_keys is per connection, so check it through the store's behaviour.
    let store = Store::open(&path).expect("test setup");
    assert!(
        store
            .create_thread(&NewThread {
                project_id: 12345,
                title: "t",
                cwd: Path::new("/x"),
                account_id: "acct",
                status: "idle"
            })
            .is_err()
    );
}

#[test]
fn project_crud() {
    let store = Store::open_in_memory().expect("test setup");
    let a = store
        .create_project("a", Path::new("/work/a"))
        .expect("test setup");
    let b = store
        .create_project("b", Path::new("/work/b"))
        .expect("test setup");
    assert_eq!(a.name, "a");
    assert_eq!(a.path, Path::new("/work/a"));
    assert!(a.created_at > 0);

    assert_eq!(store.project(a.id).expect("test setup"), Some(a.clone()));
    assert_eq!(
        store.list_projects().expect("test setup"),
        vec![a.clone(), b.clone()]
    );

    // path is UNIQUE
    assert!(store.create_project("dup", Path::new("/work/a")).is_err());

    store.rename_project(a.id, "renamed").expect("test setup");
    assert_eq!(
        store
            .project(a.id)
            .expect("test setup")
            .expect("test setup")
            .name,
        "renamed"
    );

    store.delete_project(a.id).expect("test setup");
    assert_eq!(store.project(a.id).expect("test setup"), None);
    assert_eq!(store.list_projects().expect("test setup"), vec![b]);

    assert!(store.rename_project(a.id, "gone").is_err());
    assert!(store.delete_project(a.id).is_err());
}

#[test]
fn thread_crud() {
    let (store, pid) = store_with_project();
    let t = store
        .create_thread(&NewThread {
            project_id: pid,
            title: "first",
            cwd: Path::new("/work/demo"),
            account_id: "work",
            status: "idle",
        })
        .expect("test setup");
    assert_eq!(t.project_id, pid);
    assert_eq!(t.title, "first");
    assert_eq!(t.cwd, Path::new("/work/demo"));
    assert_eq!(t.account_id, "work");
    assert_eq!(t.agent_session_id, None);
    assert_eq!(t.status, "idle");
    assert_eq!(t.created_at, t.updated_at);
    assert_eq!(store.thread(t.id).expect("test setup"), Some(t.clone()));

    tick();
    store.set_thread_title(t.id, "renamed").expect("test setup");
    store
        .set_thread_status(t.id, "running")
        .expect("test setup");
    store
        .set_agent_session_id(t.id, Some("sess-1"))
        .expect("test setup");
    let got = store.thread(t.id).expect("test setup").expect("test setup");
    assert_eq!(got.title, "renamed");
    assert_eq!(got.status, "running");
    assert_eq!(got.agent_session_id.as_deref(), Some("sess-1"));
    assert_eq!(got.account_id, "work");
    assert!(got.updated_at > t.updated_at);

    store.set_agent_session_id(t.id, None).expect("test setup");
    assert_eq!(
        store
            .thread(t.id)
            .expect("test setup")
            .expect("test setup")
            .agent_session_id,
        None
    );

    store.delete_thread(t.id).expect("test setup");
    assert_eq!(store.thread(t.id).expect("test setup"), None);
    assert!(store.set_thread_status(t.id, "x").is_err());
    assert!(store.delete_thread(t.id).is_err());
}

#[test]
fn list_threads_orders_by_updated_desc() {
    let (mut store, pid) = store_with_project();
    let other = store
        .create_project("other", Path::new("/work/o"))
        .expect("test setup");
    let cwd = Path::new("/work/demo");
    let t1 = store
        .create_thread(&NewThread {
            project_id: pid,
            title: "1",
            cwd,
            account_id: "a",
            status: "idle",
        })
        .expect("test setup");
    tick();
    let t2 = store
        .create_thread(&NewThread {
            project_id: pid,
            title: "2",
            cwd,
            account_id: "a",
            status: "idle",
        })
        .expect("test setup");
    tick();
    store
        .create_thread(&NewThread {
            project_id: other.id,
            title: "elsewhere",
            cwd,
            account_id: "a",
            status: "idle",
        })
        .expect("test setup");

    let ids = |s: &Store| -> Vec<i64> {
        s.list_threads(pid)
            .expect("test setup")
            .into_iter()
            .map(|t| t.id)
            .collect()
    };
    assert_eq!(ids(&store), [t2.id, t1.id]);

    tick();
    store.append_event(t1.id, "msg", "{}").expect("test setup");
    assert_eq!(ids(&store), [t1.id, t2.id]);

    tick();
    store.set_thread_status(t2.id, "done").expect("test setup");
    assert_eq!(ids(&store), [t2.id, t1.id]);
}

#[test]
fn seq_monotonic_per_thread() {
    let (mut store, pid) = store_with_project();
    let cwd = Path::new("/work/demo");
    let a = store
        .create_thread(&NewThread {
            project_id: pid,
            title: "a",
            cwd,
            account_id: "x",
            status: "idle",
        })
        .expect("test setup");
    let b = store
        .create_thread(&NewThread {
            project_id: pid,
            title: "b",
            cwd,
            account_id: "x",
            status: "idle",
        })
        .expect("test setup");

    let seqs: Vec<i64> = (0..3)
        .map(|_| store.append_event(a.id, "k", "{}").expect("test setup").seq)
        .collect();
    assert_eq!(seqs, [1, 2, 3]);
    assert_eq!(
        store.append_event(b.id, "k", "{}").expect("test setup").seq,
        1
    );
    assert_eq!(
        store.append_event(a.id, "k", "{}").expect("test setup").seq,
        4
    );

    assert!(store.append_event(9999, "k", "{}").is_err());
}

#[test]
fn events_since_is_incremental() {
    let (mut store, pid) = store_with_project();
    let t = store
        .create_thread(&NewThread {
            project_id: pid,
            title: "t",
            cwd: Path::new("/work/demo"),
            account_id: "x",
            status: "idle",
        })
        .expect("test setup");
    for i in 0..5 {
        store
            .append_event(t.id, "msg", &format!(r#"{{"i":{i}}}"#))
            .expect("test setup");
    }

    let all = store.events_since(t.id, 0).expect("test setup");
    assert_eq!(all.len(), 5);
    assert_eq!(all[0].thread_id, t.id);
    assert_eq!(all[0].kind, "msg");
    assert_eq!(all[0].payload_json, r#"{"i":0}"#);
    assert!(all[0].created_at > 0);

    let tail = store.events_since(t.id, 3).expect("test setup");
    assert_eq!(
        tail.iter().map(|e| e.seq).collect::<Vec<_>>(),
        [4, 5],
        "only events after the given seq, in order"
    );
    assert!(store.events_since(t.id, 5).expect("test setup").is_empty());
}

#[test]
fn project_delete_restricted_thread_delete_cascades() {
    let (mut store, pid) = store_with_project();
    let t = store
        .create_thread(&NewThread {
            project_id: pid,
            title: "t",
            cwd: Path::new("/work/demo"),
            account_id: "x",
            status: "idle",
        })
        .expect("test setup");
    store.append_event(t.id, "msg", "{}").expect("test setup");

    // RESTRICT: a project with threads can't be deleted, and nothing is lost.
    assert!(store.delete_project(pid).is_err());
    assert!(store.project(pid).expect("test setup").is_some());
    assert!(store.thread(t.id).expect("test setup").is_some());

    // CASCADE: deleting a thread takes its events with it.
    store.delete_thread(t.id).expect("test setup");
    assert!(store.events_since(t.id, 0).expect("test setup").is_empty());
    store.delete_project(pid).expect("test setup");
}

#[test]
fn file_db_persists_across_reopen() {
    let dir = tempfile::tempdir().expect("test setup");
    let path = dir.path().join("store.db");
    let (pid, tid) = {
        let mut store = Store::open(&path).expect("test setup");
        let p = store
            .create_project("demo", Path::new("/work/demo"))
            .expect("test setup");
        let t = store
            .create_thread(&NewThread {
                project_id: p.id,
                title: "t",
                cwd: Path::new("/work/demo"),
                account_id: "work",
                status: "idle",
            })
            .expect("test setup");
        store.append_event(t.id, "msg", "{}").expect("test setup");
        (p.id, t.id)
    };

    let mut store = Store::open(&path).expect("test setup");
    assert_eq!(
        store
            .project(pid)
            .expect("test setup")
            .expect("test setup")
            .name,
        "demo"
    );
    assert_eq!(
        store
            .thread(tid)
            .expect("test setup")
            .expect("test setup")
            .account_id,
        "work"
    );
    assert_eq!(store.events_since(tid, 0).expect("test setup").len(), 1);
    assert_eq!(
        store
            .append_event(tid, "msg", "{}")
            .expect("test setup")
            .seq,
        2
    );
}
