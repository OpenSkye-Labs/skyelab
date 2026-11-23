//! Settled and snoozed threads (v21): one shelf at a time, and neither reorders the thread list.

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
fn settle_and_snooze_clear_each_other_and_keep_the_order() {
    let s = Store::open_in_memory().expect("store");
    let id = thread(&s);
    let before = s.thread(id).expect("read").expect("row").updated_at;

    s.set_thread_settled(id, true).expect("settle");
    let t = s.thread(id).expect("read").expect("row");
    assert!(t.settled_at.is_some());
    assert_eq!(t.updated_at, before, "settling is not a thread update");

    s.set_thread_snoozed(id, Some(5_000)).expect("snooze");
    let t = s.thread(id).expect("read").expect("row");
    assert_eq!(t.snoozed_until, Some(5_000));
    assert_eq!(t.settled_at, None, "snoozing un-settles");

    s.set_thread_settled(id, true).expect("settle again");
    let t = s.thread(id).expect("read").expect("row");
    assert_eq!(t.snoozed_until, None, "settling wakes");

    s.set_thread_settled(id, false).expect("unsettle");
    s.set_thread_snoozed(id, Some(9)).expect("snooze");
    s.set_thread_snoozed(id, None).expect("wake");
    let t = s.thread(id).expect("read").expect("row");
    assert_eq!((t.settled_at, t.snoozed_until), (None, None));
    assert_eq!(t.updated_at, before);
    assert!(s.set_thread_settled(id + 1, true).is_err(), "unknown id");
}
