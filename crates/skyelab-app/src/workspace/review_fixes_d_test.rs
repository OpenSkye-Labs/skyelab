//! Review finding: the sidebar filter must follow threads that change after its search ran.

use super::manage_harness::rig;
use super::threads_test::wait_until;
use gpui::TestAppContext;

#[gpui::test]
fn filter_lists_a_matching_thread_added_after_the_search(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    r.thread("alpha", cx);
    let search =
        r.ws.read_with(cx, |ws, cx| ws.sidebar.read(cx).search.clone());
    search.update_in(cx, |s, window, cx| s.focus(window, cx));
    cx.simulate_input("alp");
    cx.run_until_parked();
    let ws = r.ws.clone();
    wait_until(cx, "the store search answered", |cx| {
        ws.read_with(cx, |ws, _| {
            ws.thread_search
                .as_ref()
                .is_some_and(|(_, ids)| ids.is_some())
        })
    });
    assert_eq!(r.listed(cx), ["alpha"]);
    // A new thread whose title matches the filter that is still typed in.
    r.thread("alpine", cx);
    wait_until(cx, "the new match listed", |cx| {
        r.listed(cx).iter().any(|t| t == "alpine")
    });
}

#[gpui::test]
fn deleting_again_after_an_undo_gets_a_full_undo_window(cx: &mut TestAppContext) {
    use super::manage::{ManageThread, ThreadOp, UNDO_WINDOW};
    let (r, cx) = rig(cx);
    let id = r.thread("twice", cx);
    let half = UNDO_WINDOW / 2 + std::time::Duration::from_secs(1);
    cx.dispatch_action(ManageThread {
        id,
        op: ThreadOp::Delete,
    });
    cx.run_until_parked();
    r.ws.update_in(cx, |ws, window, cx| ws.undelete_thread(id, window, cx));
    cx.run_until_parked();
    cx.executor().advance_clock(half);
    cx.dispatch_action(ManageThread {
        id,
        op: ThreadOp::Delete,
    });
    cx.run_until_parked();
    cx.executor().advance_clock(half);
    cx.run_until_parked();
    assert!(
        r.row(id).is_some(),
        "the first delete's timer must not purge the second"
    );
    cx.executor().advance_clock(UNDO_WINDOW);
    cx.run_until_parked();
    assert!(r.row(id).is_none());
}

#[gpui::test]
fn a_thread_of_another_project_with_a_worktree_is_not_deleted(cx: &mut TestAppContext) {
    use super::manage::{ManageThread, ThreadOp};
    let (r, cx) = rig(cx);
    let id = r.thread("wt", cx);
    r.f.store
        .lock()
        .expect("lock")
        .set_thread_worktree(id, Some(&r.f.project_dir.join("wt")), Some("b"))
        .expect("worktree");
    r.reload(cx);
    r.open(id, cx);
    // The sidebar moves on: the thread's row is no longer in the listed threads.
    r.ws.update(cx, |ws, _| ws.threads.clear());
    cx.dispatch_action(ManageThread {
        id,
        op: ThreadOp::Delete,
    });
    cx.run_until_parked();
    let row = r.row(id).expect("row");
    assert!(row.worktree_path.is_some());
    assert!(
        r.ws.read_with(cx, |ws, cx| ws.open_view(id, cx).is_some()),
        "not closed or deleted"
    );
}
