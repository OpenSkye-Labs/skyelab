//! A thread the last run left starting or running has no agent once the app starts again: opening the store
//! marks it interrupted, so it no longer holds a child slot or reads as busy.

use skyelab_store::{NewThread, Store};
use std::path::Path;

fn thread(store: &Store, project_id: i64, status: &str, parent: Option<i64>) -> i64 {
    let id = store
        .create_thread(&NewThread {
            project_id,
            title: status,
            cwd: Path::new("/work/demo"),
            account_id: "acme",
            status,
        })
        .expect("thread")
        .id;
    store.set_thread_parent(id, parent).expect("parent");
    id
}

#[test]
fn reopening_marks_starting_and_running_threads_interrupted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("skyelab.db");
    let (parent, children, starting, running, idle, stopped) = {
        let store = Store::open(&path).expect("create");
        let p = store
            .create_project("demo", Path::new("/work/demo"))
            .expect("project");
        let parent = thread(&store, p.id, "running", None);
        let children: Vec<i64> = (0..4)
            .map(|_| thread(&store, p.id, "running", Some(parent)))
            .collect();
        (
            parent,
            children,
            thread(&store, p.id, "starting", None),
            thread(&store, p.id, "running", None),
            thread(&store, p.id, "idle", None),
            thread(&store, p.id, "stopped", None),
        )
    };

    let mut store = Store::open(&path).expect("reopen");
    let status = |s: &Store, id| s.thread(id).expect("read").expect("row").status;
    assert_eq!(status(&store, starting), "interrupted");
    assert_eq!(status(&store, running), "interrupted");
    assert_eq!(status(&store, parent), "interrupted");
    assert_eq!(status(&store, idle), "idle", "idle rows are left alone");
    assert_eq!(status(&store, stopped), "stopped");
    let recovered: Vec<i64> = store.take_recovered().into_iter().map(|t| t.id).collect();
    assert!(recovered.contains(&starting));
    assert!(recovered.contains(&running));
    assert!(recovered.contains(&parent));
    assert!(!recovered.contains(&idle));
    assert!(!recovered.contains(&stopped));
    assert!(store.take_recovered().is_empty(), "handed over once");
    let busy = store
        .child_threads(parent)
        .expect("children")
        .iter()
        .filter(|t| matches!(t.status.as_str(), "starting" | "running"))
        .count();
    assert_eq!(busy, 0, "no stale child holds a slot");
    assert_eq!(children.len(), 4);
}
