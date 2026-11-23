//! Acceptance (package D): rename, pin, archive and delete threads, with undo, and the refusals.

use super::manage::{ManageThread, ThreadOp, UNDO_WINDOW};
use super::manage_harness::rig;
use gpui::{TestAppContext, VisualTestContext};

fn act(id: i64, op: ThreadOp, cx: &mut VisualTestContext) {
    cx.dispatch_action(ManageThread { id, op });
    cx.run_until_parked();
}

const SELECT_ALL: &str = if cfg!(target_os = "macos") {
    "cmd-a"
} else {
    "ctrl-a"
};

#[gpui::test]
fn rename_through_the_dialog(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let id = r.thread("old name", cx);
    act(id, ThreadOp::Rename, cx);
    assert!(r.dialog_open(cx), "rename asks for the new name");
    cx.simulate_keystrokes(SELECT_ALL);
    cx.simulate_input("New name");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!r.dialog_open(cx));
    assert_eq!(r.row(id).expect("row").title, "New name");
    assert_eq!(r.listed(cx), ["New name"]);
}

#[gpui::test]
fn pin_and_archive_with_undo(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let a = r.thread("a", cx);
    let b = r.thread("b", cx);
    act(a, ThreadOp::Pin, cx);
    assert!(r.row(a).expect("row").pinned_at.is_some());
    assert_eq!(r.listed(cx), ["a", "b"]);

    act(b, ThreadOp::Archive, cx);
    assert!(r.row(b).expect("row").archived_at.is_some());
    assert_eq!(r.listed(cx), ["a"]);
    assert_eq!(r.archived(cx), ["b"]);
    assert_eq!(r.notices(cx), ["Archived “b”"]);
    r.ws.update_in(cx, |ws, window, cx| ws.archive_thread(b, false, window, cx));
    cx.run_until_parked();
    assert!(r.row(b).expect("row").archived_at.is_none(), "undo");
    assert_eq!(r.listed(cx), ["a", "b"]);
}

#[gpui::test]
fn delete_hides_undo_restores_and_the_window_purges(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let id = r.thread("doomed", cx);
    act(id, ThreadOp::Delete, cx);
    assert!(r.listed(cx).is_empty());
    assert_eq!(r.notices(cx), ["Deleted “doomed”"]);
    r.ws.update_in(cx, |ws, window, cx| ws.undo_delete(id, false, window, cx));
    cx.run_until_parked();
    assert_eq!(r.listed(cx), ["doomed"], "undo brings it back");

    act(id, ThreadOp::Delete, cx);
    assert!(r.row(id).is_some(), "kept for the undo window");
    cx.executor().advance_clock(UNDO_WINDOW);
    cx.run_until_parked();
    assert!(r.row(id).is_none(), "purged once the window passed");
}

#[gpui::test]
fn a_working_thread_or_one_with_a_worktree_refuses(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let busy = r.thread("busy", cx);
    let view = r.open(busy, cx);
    view.update(cx, |v, _| v.starting = true);
    act(busy, ThreadOp::Archive, cx);
    act(busy, ThreadOp::Delete, cx);
    let row = r.row(busy).expect("row");
    assert!(row.archived_at.is_none());
    assert_eq!(r.notices(cx).len(), 2, "both refusals say why");
    view.update(cx, |v, _| v.starting = false);

    let wt = r.thread("worktree", cx);
    r.f.store
        .lock()
        .expect("lock")
        .set_thread_worktree(wt, Some(&r.f.project_dir.join("wt")), Some("b"))
        .expect("worktree");
    r.reload(cx);
    act(wt, ThreadOp::Delete, cx);
    assert!(
        r.listed(cx).contains(&"worktree".to_owned()),
        "not orphaning a worktree"
    );
}
