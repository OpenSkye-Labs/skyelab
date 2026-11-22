//! Schema 3: a delegated thread records its parent.

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

#[test]
fn v2_database_upgrades_with_no_parents() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("store.db");
    {
        let store = Store::open(&path).expect("create");
        let p = store
            .create_project("demo", Path::new("/work/demo"))
            .expect("project");
        thread(&store, p.id, "old");
    }
    // Roll the file back to schema 2, as an older build left it.
    let conn = Connection::open(&path).expect("open raw");
    conn.execute_batch("DROP INDEX threads_by_parent; ALTER TABLE threads DROP COLUMN parent_thread_id; PRAGMA user_version = 2;")
        .expect("downgrade");
    drop(conn);
    let store = Store::open(&path).expect("migrates");
    let t = store.thread(1).expect("read").expect("kept");
    assert_eq!(t.title, "old");
    assert_eq!(t.parent_thread_id, None);
    let v: i64 = Connection::open(&path)
        .expect("raw")
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("version");
    assert_eq!(v, 3);
}
