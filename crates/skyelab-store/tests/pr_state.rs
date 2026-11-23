//! Schema 14: the linked pull request's last read state, kept so an ended PR isn't read again after a restart.

use skyelab_store::{NewThread, Store};
use std::path::Path;

const URL: &str = "https://github.com/acme/api/pull/7";

#[test]
fn pr_state_survives_reopen_and_goes_with_the_link() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("store.db");
    let store = Store::open(&path).expect("open");
    let p = store
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    let id = store
        .create_thread(&NewThread {
            project_id: p.id,
            title: "t",
            cwd: Path::new("/work/demo"),
            account_id: "acme",
            status: "idle",
        })
        .expect("thread")
        .id;
    store.set_thread_pr_url(id, Some(URL)).expect("link");
    let updated = store.thread(id).expect("read").expect("row").updated_at;
    std::thread::sleep(std::time::Duration::from_millis(5));
    store.set_thread_pr_state(id, URL, "merged").expect("state");
    drop(store);

    let store = Store::open(&path).expect("reopen");
    let t = store.thread(id).expect("read").expect("row");
    assert_eq!(t.pr_state.as_deref(), Some("merged"));
    assert_eq!(
        t.updated_at, updated,
        "a background read doesn't reorder the sidebar"
    );
    assert_eq!(
        store.list_threads(p.id).expect("list")[0]
            .pr_state
            .as_deref(),
        Some("merged")
    );

    // A read for a PR that was relinked meanwhile is dropped.
    store
        .set_thread_pr_url(id, Some("https://github.com/acme/api/pull/8"))
        .expect("relink");
    assert_eq!(store.thread(id).expect("read").expect("row").pr_state, None);
    store
        .set_thread_pr_state(id, URL, "closed")
        .expect("stale write");
    assert_eq!(store.thread(id).expect("read").expect("row").pr_state, None);

    store
        .set_thread_pr_state(id, "https://github.com/acme/api/pull/8", "open")
        .expect("state");
    store.set_thread_pr_url(id, None).expect("unlink");
    assert_eq!(store.thread(id).expect("read").expect("row").pr_state, None);
}
