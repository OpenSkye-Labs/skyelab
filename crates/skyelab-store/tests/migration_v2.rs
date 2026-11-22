//! A database written by the first schema upgrades in place and keeps its threads.

use rusqlite::Connection;
use skyelab_store::Store;
use std::path::Path;

/// The schema as the first release shipped it (user_version 1), with one project and one thread.
fn write_v1(path: &Path) {
    let conn = Connection::open(path).expect("open v1");
    conn.execute_batch(
        "
        CREATE TABLE projects (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
            path TEXT NOT NULL UNIQUE, created_at INTEGER NOT NULL);
        CREATE TABLE threads (id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
            title TEXT NOT NULL, cwd TEXT NOT NULL, account_id TEXT NOT NULL, agent_session_id TEXT,
            status TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
        CREATE INDEX threads_by_project ON threads(project_id, updated_at DESC);
        CREATE TABLE events (thread_id INTEGER NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
            seq INTEGER NOT NULL, kind TEXT NOT NULL, payload_json TEXT NOT NULL,
            created_at INTEGER NOT NULL, PRIMARY KEY (thread_id, seq)) WITHOUT ROWID;
        INSERT INTO projects (name, path, created_at) VALUES ('demo', '/work/demo', 1);
        INSERT INTO threads (project_id, title, cwd, account_id, status, created_at, updated_at)
            VALUES (1, 'old', '/work/demo', 'acme', 'exited', 1, 1);
        PRAGMA user_version = 1;
        ",
    )
    .expect("v1 schema");
}

#[test]
fn v1_database_migrates_to_v2_and_keeps_threads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("store.db");
    write_v1(&path);

    let store = Store::open(&path).expect("open migrates");
    let thread = store.thread(1).expect("read").expect("thread kept");
    assert_eq!(thread.title, "old");
    assert_eq!(thread.account_id, "acme");
    assert_eq!(thread.worktree_path, None);
    assert_eq!(thread.worktree_branch, None);

    let wt = Path::new("/work/demo/.skyelab/worktrees/fix-1");
    store
        .set_thread_worktree(1, Some(wt), Some("skyelab/fix-1"))
        .expect("set worktree");
    let thread = store.thread(1).expect("read").expect("thread");
    assert_eq!(thread.worktree_path.as_deref(), Some(wt));
    assert_eq!(thread.worktree_branch.as_deref(), Some("skyelab/fix-1"));
    drop(store);

    let version: i64 = Connection::open(&path)
        .expect("reopen")
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("version");
    assert!(version >= 2, "{version}");

    // Discarding clears the path and keeps the branch; reopening an upgraded file is a no-op.
    let store = Store::open(&path).expect("reopen at v2");
    store
        .set_thread_worktree(1, None, Some("skyelab/fix-1"))
        .expect("clear");
    let thread = store.list_threads(1).expect("list").remove(0);
    assert_eq!(thread.worktree_path, None);
    assert_eq!(thread.worktree_branch.as_deref(), Some("skyelab/fix-1"));
}
