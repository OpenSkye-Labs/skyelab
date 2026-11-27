//! Criterion 1: a linked caller whose login matches starts a thread here, under this machine's own binding for the
//! folder, with a `delegated_from` row naming the caller's node, parent and depth.

use super::remote_delegation_harness::*;
use gpui::TestAppContext;

#[gpui::test]
fn a_linked_start_runs_under_the_folders_account_and_records_its_origin(cx: &mut TestAppContext) {
    let (b, cx) = b_side(
        Setup {
            link_to: Some("work"),
            ..Setup::default()
        },
        cx,
    );
    let mut from = caller();
    from.depth = 1;
    from.store_id = "claimed-in-the-message".into();
    let id = started(&b, &from, start_op(b.project("work-a"), "r1"), cx);
    let (row, origin) = {
        let s = b.store.lock().expect("lock");
        (s.thread(id).expect("read").expect("row"), s.delegated_from(id).expect("read").expect("origin"))
    };
    assert_eq!(row.account_id, "work", "the folder's binding here decides");
    assert_eq!(row.parent_thread_id, None, "no local parent");
    assert_eq!(origin.origin_node, A_NODE, "the node the connection proved");
    assert_eq!(origin.origin_machine, A_NAME);
    assert_eq!(origin.origin_store, "claimed-in-the-message");
    assert_eq!(origin.parent_thread_id, 7);
    assert_eq!(origin.origin_account, "x");
    assert_eq!(origin.depth, 2, "the caller's depth + 1");
    assert_eq!(origin.request_id.as_deref(), Some("r1"));
    let env = wait_for_agent(&b, id, cx);
    let work = b.account_dir("work");
    assert!(env.contains(work.to_str().expect("utf-8")), "launched as work: {env}");
    assert_ne!(row.permission_mode, "bypassPermissions", "never Full access");
}
