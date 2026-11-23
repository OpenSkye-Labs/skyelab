//! Acceptance: project scripts are stored per project, and their migration applies on its own to a database at
//! main's version 5 (it is planned as version 8 and must be movable to that slot).

use rusqlite::Connection;
use skyelab_store::Store;
use std::path::Path;

#[test]
fn scripts_are_added_listed_and_deleted_per_project() {
    let store = Store::open_in_memory().expect("store");
    let a = store.create_project("a", Path::new("/a")).expect("a");
    let b = store.create_project("b", Path::new("/b")).expect("b");
    let install = store
        .add_project_script(a.id, "Install", "npm install", true)
        .expect("add");
    let dev = store
        .add_project_script(a.id, "Dev", "npm run dev", false)
        .expect("add");
    store
        .add_project_script(b.id, "Test", "cargo test", false)
        .expect("add");
    let listed = store.project_scripts(a.id).expect("list");
    assert_eq!(listed, [install.clone(), dev.clone()]);
    assert!(listed[0].setup && !listed[1].setup);
    store.delete_project_script(install.id).expect("delete");
    assert_eq!(store.project_scripts(a.id).expect("list"), [dev]);
    assert!(store.delete_project_script(install.id).is_err());
    assert_eq!(store.project_scripts(b.id).expect("list").len(), 1);
    // A project without threads can go, and its scripts with it.
    store.delete_project(b.id).expect("delete project");
    assert!(store.project_scripts(b.id).expect("list").is_empty());
    assert!(
        store.add_project_script(999, "x", "y", false).is_err(),
        "no such project"
    );
}

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
        PRAGMA user_version = 5;
        ",
    )
    .expect("v5 schema");
}

#[test]
fn a_v5_database_gains_project_scripts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("store.db");
    write_v5(&path);
    let store = Store::open(&path).expect("open migrates");
    assert!(store.project_scripts(1).expect("list").is_empty());
    store
        .add_project_script(1, "Install", "pnpm i", true)
        .expect("add");
    drop(store);
    let store = Store::open(&path).expect("reopen");
    assert_eq!(store.project_scripts(1).expect("list").len(), 1);
}
