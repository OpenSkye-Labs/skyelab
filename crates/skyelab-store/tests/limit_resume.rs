//! T3's Resume at reset (v27): the schedule survives a restart, fires once, and archiving cancels it.

use skyelab_store::{NewThread, Store};
use std::path::Path;

fn thread(s: &Store) -> i64 {
    let p = s.create_project("p", Path::new("/tmp/p")).expect("project");
    s.create_thread(&NewThread {
        project_id: p.id,
        title: "t",
        cwd: Path::new("/tmp/p"),
        account_id: "a",
        status: "idle",
    })
    .expect("thread")
    .id
}

#[test]
fn a_scheduled_resume_survives_a_restart() {
    let dir = tempfile::tempdir().expect("dir");
    let db = dir.path().join("skyelab.db");
    let id = {
        let s = Store::open(&db).expect("open");
        let id = thread(&s);
        s.set_limit_resume(id, Some(7_000)).expect("schedule");
        id
    };
    let s = Store::open(&db).expect("reopen");
    assert_eq!(
        s.thread(id).expect("read").expect("row").limit_resume_at,
        Some(7_000)
    );
    let listed = s.list_all_threads().expect("list");
    assert_eq!(
        listed
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.limit_resume_at),
        Some(7_000)
    );
}

#[test]
fn a_resume_is_claimed_once_and_only_when_due() {
    let s = Store::open_in_memory().expect("store");
    let id = thread(&s);
    s.set_limit_resume(id, Some(7_000)).expect("schedule");
    assert!(
        !s.take_limit_resume(id, 6_999).expect("early"),
        "not before the reset"
    );
    assert!(s.take_limit_resume(id, 7_000).expect("due"), "at the reset");
    assert!(!s.take_limit_resume(id, 8_000).expect("again"), "only once");
    assert_eq!(
        s.thread(id).expect("read").expect("row").limit_resume_at,
        None
    );
}

#[test]
fn archiving_or_deleting_cancels_the_resume() {
    let s = Store::open_in_memory().expect("store");
    let id = thread(&s);
    s.set_limit_resume(id, Some(7_000)).expect("schedule");
    s.set_thread_archived(id, true).expect("archive");
    assert!(!s.take_limit_resume(id, 9_000).expect("claim"));
    s.set_thread_archived(id, false).expect("unarchive");
    assert_eq!(
        s.thread(id).expect("read").expect("row").limit_resume_at,
        None
    );

    s.set_limit_resume(id, Some(7_000)).expect("schedule");
    s.set_thread_deleted(id, true).expect("delete");
    s.set_thread_deleted(id, false).expect("undo");
    assert_eq!(
        s.thread(id).expect("read").expect("row").limit_resume_at,
        None
    );
}
