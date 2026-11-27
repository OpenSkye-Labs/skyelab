//! Criterion 6: a start repeated with the same request id, before and after B's store is reopened, returns the
//! same thread, with one row and one agent spawned.

use super::delegate_remote::RemoteStarts;
use super::remote_delegation_harness::*;
use super::*;
use gpui::TestAppContext;

fn spawns(b: &B, id: i64) -> usize {
    lock(&b.store)
        .and_then(|s| s.events_since(id, 0))
        .expect("events")
        .iter()
        .filter(|e| e.payload_json.contains("fake_env"))
        .count()
}

#[gpui::test]
fn a_repeated_start_finds_the_first_thread(cx: &mut TestAppContext) {
    let (b, cx) = b_side(
        Setup {
            link_to: Some("work"),
            disk_store: true,
            ..Setup::default()
        },
        cx,
    );
    let op = || start_op(b.project("work-a"), "same-request");
    let id = started(&b, &caller(), op(), cx);
    assert_eq!(started(&b, &caller(), op(), cx), id, "a retry while it starts");
    wait_for_agent(&b, id, cx);
    assert_eq!(started(&b, &caller(), op(), cx), id, "a retry once it runs");
    assert_eq!(b.thread_count(), 1);

    let db = b.db.clone().expect("a store file");
    *b.store.lock().expect("lock") = Store::open(&db).expect("reopen");
    cx.update(|_, cx| cx.set_global(RemoteStarts::default()));
    assert_eq!(started(&b, &caller(), op(), cx), id, "a retry after a restart");
    assert_eq!(b.thread_count(), 1, "one row");
    assert_eq!(spawns(&b, id), 1, "one agent");
    let mut other_parent = caller();
    other_parent.thread_id = 8;
    assert_ne!(started(&b, &other_parent, op(), cx), id, "request ids are per caller");
}
