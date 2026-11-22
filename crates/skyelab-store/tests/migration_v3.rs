//! Acceptance: schema v3 adds `pr_url` to threads; a v2 database upgrades in place.

use rusqlite::Connection;
use skyelab_store::Store;
use std::path::Path;

fn write_v2(path: &Path) {
    let conn = Connection::open(path).expect("open v2");
    conn.execute_batch(
        "
        CREATE TABLE projects (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
            path TEXT NOT NULL UNIQUE, created_at INTEGER NOT NULL);
        CREATE TABLE threads (id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
            title TEXT NOT NULL, cwd TEXT NOT NULL, account_id TEXT NOT NULL, agent_session_id TEXT,
            status TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
            worktree_path TEXT, worktree_branch TEXT);
        CREATE INDEX threads_by_project ON threads(project_id, updated_at DESC);
        CREATE TABLE events (thread_id INTEGER NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
            seq INTEGER NOT NULL, kind TEXT NOT NULL, payload_json TEXT NOT NULL,
            created_at INTEGER NOT NULL, PRIMARY KEY (thread_id, seq)) WITHOUT ROWID;
        INSERT INTO projects (name, path, created_at) VALUES ('demo', '/work/demo', 1);
        INSERT INTO threads (project_id, title, cwd, account_id, status, created_at, updated_at)
            VALUES (1, 'old', '/work/demo', 'acme', 'exited', 1, 1);
        PRAGMA user_version = 2;
        ",
    )
    .expect("v2 schema");
}

#[test]
fn v2_database_gains_pr_url() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("store.db");
    write_v2(&path);

    let store = Store::open(&path).expect("open migrates");
    let thread = store.thread(1).expect("read").expect("thread kept");
    assert_eq!(thread.title, "old");
    assert_eq!(thread.pr_url, None);

    let url = "https://github.com/acme/api/pull/7";
    store.set_thread_pr_url(1, Some(url)).expect("set pr url");
    drop(store);

    let version: i64 = Connection::open(&path)
        .expect("reopen")
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("version");
    assert!(version >= 3, "{version}");

    let store = Store::open(&path).expect("reopen");
    assert_eq!(
        store.list_threads(1).expect("list")[0].pr_url.as_deref(),
        Some(url)
    );
    assert!(
        store.set_thread_pr_url(99, Some(url)).is_err(),
        "no such thread"
    );
}
