//! Schema 5: a delegated thread records its parent.

mod common;

use rusqlite::Connection;
use skyelab_store::{NewThread, Store};
use std::path::Path;

fn thread(store: &Store, project_id: i64, title: &str) -> i64 {
    store
        .create_thread(&NewThread {
            project_id,
            title,
            cwd: Path::new("/work/demo"),
            account_id: "acme",
            status: "starting",
        })
        .expect("thread")
        .id
}

#[test]
fn parent_round_trips_and_children_list() {
    let store = Store::open_in_memory().expect("open");
    let p = store
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    let parent = thread(&store, p.id, "parent");
    let a = thread(&store, p.id, "a");
    let b = thread(&store, p.id, "b");
    let _other = thread(&store, p.id, "other");
    assert_eq!(
        store.thread(a).expect("read").expect("a").parent_thread_id,
        None
    );
    store.set_thread_parent(a, Some(parent)).expect("set a");
    store.set_thread_parent(b, Some(parent)).expect("set b");
    assert_eq!(
        store.thread(a).expect("read").expect("a").parent_thread_id,
        Some(parent)
    );
    let mut kids: Vec<i64> = store
        .child_threads(parent)
        .expect("children")
        .iter()
        .map(|t| t.id)
        .collect();
    kids.sort();
    assert_eq!(kids, vec![a, b]);
    assert!(store.child_threads(a).expect("none").is_empty());
    assert!(
        store.set_thread_parent(a, Some(9999)).is_err(),
        "parent must exist"
    );
    // Deleting a parent keeps its children, unlinked.
    store.delete_thread(parent).expect("delete parent");
    assert_eq!(
        store.thread(a).expect("read").expect("a").parent_thread_id,
        None
    );
}

/// A file at `version` as an older build left it: one thread with the data that version can hold, newer columns dropped.
fn downgraded(dir: &Path, version: i64) -> std::path::PathBuf {
    let path = dir.join("store.db");
    {
        let store = Store::open(&path).expect("create");
        let p = store
            .create_project("demo", Path::new("/work/demo"))
            .expect("project");
        let t = thread(&store, p.id, "old");
        if version >= 3 {
            store
                .set_thread_worktree_base(t, Some("abc123"))
                .expect("base");
        }
        if version >= 4 {
            store
                .set_thread_pr_url(t, Some("https://github.com/o/r/pull/1"))
                .expect("pr url");
        }
    }
    let conn = Connection::open(&path).expect("open raw");
    conn.execute_batch(common::UNDO_V13).expect("drop v13");
    conn.execute_batch(common::UNDO_V11).expect("drop v11");
    conn.execute_batch("DROP TRIGGER usage_reading_insert; DROP TABLE usage_readings;")
        .expect("drop v10");
    conn.execute_batch(
        "ALTER TABLE threads DROP COLUMN model; ALTER TABLE threads DROP COLUMN effort;
         ALTER TABLE projects DROP COLUMN default_model; ALTER TABLE projects DROP COLUMN default_effort;",
    )
    .expect("drop v9");
    conn.execute_batch("DROP TABLE project_scripts;")
        .expect("drop the scripts step");
    conn.execute_batch(
        "DROP TRIGGER event_text_insert; DROP TRIGGER event_text_thread_delete; DROP TABLE event_text;
         ALTER TABLE threads DROP COLUMN pinned_at; ALTER TABLE threads DROP COLUMN archived_at;
         ALTER TABLE threads DROP COLUMN deleted_at; ALTER TABLE threads DROP COLUMN unread;
         ALTER TABLE projects DROP COLUMN removed_at;",
    )
    .expect("drop v7");
    conn.execute_batch("ALTER TABLE threads DROP COLUMN permission_mode;")
        .expect("drop v6");
    conn.execute_batch(
        "DROP INDEX threads_by_parent; ALTER TABLE threads DROP COLUMN parent_thread_id;",
    )
    .expect("drop v5");
    if version < 4 {
        conn.execute_batch("ALTER TABLE threads DROP COLUMN pr_url;")
            .expect("drop v4");
    }
    if version < 3 {
        conn.execute_batch("ALTER TABLE threads DROP COLUMN worktree_base;")
            .expect("drop v3");
    }
    conn.pragma_update(None, "user_version", version)
        .expect("set version");
    path
}

fn version(path: &Path) -> i64 {
    Connection::open(path)
        .expect("raw")
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("version")
}

#[test]
fn v2_database_upgrades_with_no_parents() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = downgraded(dir.path(), 2);
    let store = Store::open(&path).expect("migrates");
    let t = store.thread(1).expect("read").expect("kept");
    assert_eq!(t.title, "old");
    assert_eq!(t.worktree_base, None);
    assert_eq!(t.pr_url, None);
    assert_eq!(t.parent_thread_id, None);
    assert_eq!(version(&path), 14);
}

#[test]
fn v3_database_keeps_its_worktree_base_through_v5() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = downgraded(dir.path(), 3);
    let store = Store::open(&path).expect("migrates");
    let t = store.thread(1).expect("read").expect("kept");
    assert_eq!(t.title, "old");
    assert_eq!(t.worktree_base.as_deref(), Some("abc123"));
    assert_eq!(t.pr_url, None);
    assert_eq!(t.parent_thread_id, None);
    assert_eq!(version(&path), 14);
    // The new column works on an upgraded file.
    let p = store.thread(1).expect("read").expect("t").project_id;
    let child = thread(&store, p, "child");
    store.set_thread_parent(child, Some(1)).expect("link");
    assert_eq!(store.child_threads(1).expect("children").len(), 1);
}

#[test]
fn v4_database_keeps_worktree_base_and_pr_url_through_v5() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = downgraded(dir.path(), 4);
    let store = Store::open(&path).expect("migrates");
    let t = store.thread(1).expect("read").expect("kept");
    assert_eq!(t.title, "old");
    assert_eq!(t.worktree_base.as_deref(), Some("abc123"));
    assert_eq!(t.pr_url.as_deref(), Some("https://github.com/o/r/pull/1"));
    assert_eq!(t.parent_thread_id, None);
    assert_eq!(version(&path), 14);
    let child = thread(&store, t.project_id, "child");
    store.set_thread_parent(child, Some(1)).expect("link");
    let kids = store.child_threads(1).expect("children");
    assert_eq!(kids.len(), 1);
    assert_eq!(kids[0].parent_thread_id, Some(1));
}
