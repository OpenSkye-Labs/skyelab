//! Acceptance (package D): schema v7 adds pin, archive, unread, soft delete and project removal; a v5 database
//! upgrades in place.

use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use rusqlite::Connection;
use skyelab_store::{NewThread, Store};

fn write_v5(path: &Path) {
    let conn = Connection::open(path).expect("open v5");
    conn.execute_batch(
        "
        CREATE TABLE projects (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
            path TEXT NOT NULL UNIQUE, created_at INTEGER NOT NULL);
        CREATE TABLE threads (id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
            title TEXT NOT NULL, cwd TEXT NOT NULL, account_id TEXT NOT NULL, agent_session_id TEXT,
            status TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
            worktree_path TEXT, worktree_branch TEXT, worktree_base TEXT, pr_url TEXT,
            parent_thread_id INTEGER REFERENCES threads(id) ON DELETE SET NULL);
        CREATE INDEX threads_by_project ON threads(project_id, updated_at DESC);
        CREATE INDEX threads_by_parent ON threads(parent_thread_id);
        CREATE TABLE events (thread_id INTEGER NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
            seq INTEGER NOT NULL, kind TEXT NOT NULL, payload_json TEXT NOT NULL,
            created_at INTEGER NOT NULL, PRIMARY KEY (thread_id, seq)) WITHOUT ROWID;
        INSERT INTO projects (name, path, created_at) VALUES ('demo', '/work/demo', 1);
        INSERT INTO threads (project_id, title, cwd, account_id, status, created_at, updated_at)
            VALUES (1, 'old', '/work/demo', 'acme', 'exited', 1, 1);
        INSERT INTO events VALUES (1, 1, 'prompt', '\"find the flamingo bug\"', 1);
        PRAGMA user_version = 5;
        ",
    )
    .expect("v5 schema");
}

fn version(path: &Path) -> i64 {
    Connection::open(path)
        .expect("reopen")
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("version")
}

fn new_thread<'a>(project_id: i64, title: &'a str) -> NewThread<'a> {
    NewThread {
        project_id,
        title,
        cwd: Path::new("/work/demo"),
        account_id: "acme",
        status: "idle",
    }
}

#[test]
fn v5_database_upgrades_to_v7_and_keeps_rows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("store.db");
    write_v5(&path);

    let store = Store::open(&path).expect("open migrates");
    let t = store.thread(1).expect("read").expect("thread kept");
    assert_eq!(t.title, "old");
    assert_eq!((t.pinned_at, t.archived_at, t.unread), (None, None, false));
    // The old prompt was indexed by the migration's backfill.
    let found = store.search_threads("flamingo", None, 10).expect("search");
    assert_eq!(found.iter().map(|t| t.id).collect::<Vec<_>>(), [1]);
    drop(store);
    assert_eq!(version(&path), 7);
}

#[test]
fn pin_archive_unread_round_trip_without_reordering() {
    let store = Store::open_in_memory().expect("store");
    let p = store
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    let t = store.create_thread(&new_thread(p.id, "a")).expect("thread");
    sleep(Duration::from_millis(5));

    store.set_thread_pinned(t.id, true).expect("pin");
    store.set_thread_archived(t.id, true).expect("archive");
    store.set_thread_unread(t.id, true).expect("unread");
    let got = store.thread(t.id).expect("read").expect("row");
    assert!(got.pinned_at.is_some() && got.archived_at.is_some() && got.unread);
    assert_eq!(got.updated_at, t.updated_at, "bookkeeping never reorders");

    store.set_thread_pinned(t.id, false).expect("unpin");
    store.set_thread_archived(t.id, false).expect("unarchive");
    store.set_thread_unread(t.id, false).expect("read");
    let got = store.thread(t.id).expect("read").expect("row");
    assert_eq!(
        (got.pinned_at, got.archived_at, got.unread),
        (None, None, false)
    );
    assert!(store.set_thread_pinned(99, true).is_err(), "no such thread");
}

#[test]
fn soft_deleted_threads_hide_restore_and_purge_on_open() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("store.db");
    let store = Store::open(&path).expect("store");
    let p = store
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    let keep = store.create_thread(&new_thread(p.id, "keep")).expect("t");
    let gone = store.create_thread(&new_thread(p.id, "gone")).expect("t");

    store.set_thread_deleted(gone.id, true).expect("delete");
    let ids = |s: &Store| -> Vec<i64> {
        s.list_threads(p.id)
            .expect("list")
            .into_iter()
            .map(|t| t.id)
            .collect()
    };
    assert_eq!(ids(&store), [keep.id]);
    store.set_thread_deleted(gone.id, false).expect("undo");
    assert_eq!(ids(&store).len(), 2);

    store.set_thread_deleted(gone.id, true).expect("delete");
    // A purge only removes rows still marked deleted.
    store.purge_thread(keep.id).expect("purge is a no-op");
    assert!(store.thread(keep.id).expect("read").is_some());
    drop(store);

    let store = Store::open(&path).expect("reopen purges");
    assert!(store.thread(gone.id).expect("read").is_none());
    assert_eq!(ids(&store), [keep.id]);
}

#[test]
fn removed_projects_hide_keep_threads_and_come_back() {
    let store = Store::open_in_memory().expect("store");
    let p = store
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    let t = store.create_thread(&new_thread(p.id, "audit")).expect("t");

    store.set_project_removed(p.id, true).expect("remove");
    assert!(store.list_projects().expect("list").is_empty());
    assert!(store.thread(t.id).expect("read").is_some(), "audit kept");

    let back = store
        .readd_project(Path::new("/work/demo/"))
        .expect("readd")
        .expect("found by path");
    assert_eq!(back.id, p.id);
    assert_eq!(store.list_projects().expect("list").len(), 1);
    assert!(
        store
            .readd_project(Path::new("/elsewhere"))
            .expect("readd")
            .is_none()
    );
}

#[test]
fn unread_projects_skip_archived_and_deleted_threads() {
    let store = Store::open_in_memory().expect("store");
    let p = store
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    let q = store
        .create_project("other", Path::new("/work/other"))
        .expect("project");
    let a = store.create_thread(&new_thread(p.id, "a")).expect("thread");
    let b = store.create_thread(&new_thread(q.id, "b")).expect("thread");
    assert!(store.unread_project_ids().expect("query").is_empty());
    store.set_thread_unread(a.id, true).expect("unread");
    store.set_thread_unread(b.id, true).expect("unread");
    let mut ids = store.unread_project_ids().expect("query");
    ids.sort();
    assert_eq!(ids, [p.id, q.id]);
    store.set_thread_archived(a.id, true).expect("archive");
    store.set_thread_deleted(b.id, true).expect("delete");
    assert!(store.unread_project_ids().expect("query").is_empty());
    // A deleted thread can't be opened while its delete can still be undone; an archived one can.
    assert!(store.listed_thread(b.id).expect("read").is_none());
    assert!(store.listed_thread(a.id).expect("read").is_some());
    store.set_thread_deleted(b.id, false).expect("undelete");
    assert!(store.listed_thread(b.id).expect("read").is_some());
}
