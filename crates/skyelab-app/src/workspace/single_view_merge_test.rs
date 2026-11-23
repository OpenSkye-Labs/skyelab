//! Package D on the single page: archive and delete close the thread's view (undo shows it again), D's keys switch
//! the page, a background start is marked through D's stored unread flag, and back/forward keep their own keys.

use super::center::Page;
use super::manage::{ManageThread, ThreadOp};
use super::manage_harness::{Rig, rig};
use super::*;
use gpui::{TestAppContext, VisualTestContext};

const M: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};

fn page_thread(r: &Rig, cx: &mut VisualTestContext) -> Option<i64> {
    r.ws.read_with(cx, |ws, cx| {
        ws.current_thread()
            .and_then(|v| v.read(cx).thread.as_ref().map(|t| t.id))
    })
}

fn is_open(r: &Rig, id: i64, cx: &mut VisualTestContext) -> bool {
    r.ws.read_with(cx, |ws, cx| ws.open_view(id, cx).is_some())
}

fn undo_last(r: &Rig, op: ThreadOp, id: i64, cx: &mut VisualTestContext) {
    // The notice's Undo button runs the same call; the harness can't click a notification.
    r.ws.update_in(cx, |ws, window, cx| match op {
        ThreadOp::Archive => ws.undo_archive(id, true, window, cx),
        _ => ws.undo_delete(id, true, window, cx),
    });
    cx.run_until_parked();
}

#[gpui::test]
fn archive_and_delete_close_the_view_and_undo_shows_it_again(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    for op in [ThreadOp::Archive, ThreadOp::Delete] {
        let id = r.thread("doomed", cx);
        let view = r.open(id, cx);
        assert_eq!(page_thread(&r, cx), Some(id), "precondition: shown");
        let weak = view.downgrade();
        drop(view);
        cx.dispatch_action(ManageThread { id, op });
        cx.run_until_parked();
        assert!(!is_open(&r, id, cx), "{op:?} closes the view");
        assert_eq!(r.ws.read_with(cx, |ws, _| ws.page.clone()), Page::Welcome);
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        assert!(weak.upgrade().is_none(), "{op:?} releases the view");

        undo_last(&r, op, id, cx);
        super::threads_test::wait_until(cx, "the thread shown again", |cx| {
            page_thread(&r, cx) == Some(id)
        });
    }
}

#[gpui::test]
fn thread_keys_switch_the_page_without_dock_tabs(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let a = r.thread("a", cx);
    let b = r.thread("b", cx); // listed b, a
    cx.simulate_keystrokes(&format!("{M}-1"));
    cx.run_until_parked();
    assert_eq!(page_thread(&r, cx), Some(b));
    cx.simulate_keystrokes(&format!("{M}-shift-]"));
    cx.run_until_parked();
    assert_eq!(page_thread(&r, cx), Some(a));
    r.ws.read_with(cx, |ws, cx| {
        let dock = ws.dock.read(cx);
        for v in &ws.open_threads {
            assert!(dock.panel(v.entity_id().into()).is_none(), "no thread tab");
        }
    });
    // Back walks the pages the thread keys showed.
    cx.dispatch_action(GoBack);
    cx.run_until_parked();
    assert_eq!(page_thread(&r, cx), Some(b));
}

#[gpui::test]
fn a_background_start_uses_the_stored_unread_flag(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let shown = r.thread("shown", cx);
    r.open(shown, cx);
    let project = r.ws.read_with(cx, |ws, _| ws.projects[0].clone());
    let child = r.ws.update_in(cx, |ws, window, cx| {
        let (config, backend) = (ws.config.clone(), ws.backend.clone());
        let view = cx.new(|cx| thread::ThreadView::new(&project, config, backend, window, cx));
        view.update(cx, |v, cx| v.submit_remote("hi", window, cx))
            .expect("submitted");
        ws.add_thread_tab(view.clone(), false, window, cx);
        view
    });
    super::threads_test::wait_until(cx, "the child's row", |cx| {
        child.read_with(cx, |v, _| v.thread.is_some())
    });
    let id = child.read_with(cx, |v, _| v.thread.as_ref().expect("row").id);
    super::threads_test::wait_until(cx, "the stored flag", |_| {
        r.row(id).is_some_and(|t| t.unread)
    });
    assert_eq!(r.marks(id, cx), (false, true), "the one unread marker");
    assert_eq!(page_thread(&r, cx), Some(shown), "the page didn't change");
}

#[gpui::test]
fn back_forward_and_previous_next_have_their_own_keys(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let a = r.thread("a", cx);
    let b = r.thread("b", cx); // listed b, a
    r.open(a, cx);
    r.open(b, cx);
    // Previous thread (⌘⇧[) goes by the sidebar order: b is first, so it wraps to a.
    cx.simulate_keystrokes(&format!("{M}-shift-["));
    cx.run_until_parked();
    assert_eq!(page_thread(&r, cx), Some(a));
    // Back (⌘[ on macOS, alt-left elsewhere) goes by history: the page before was b. From the workspace, since off
    // macOS a text field keeps alt-left.
    let focus_ws = |cx: &mut VisualTestContext| {
        r.ws.update_in(cx, |ws, window, cx| ws.focus.focus(window, cx))
    };
    focus_ws(cx);
    cx.simulate_keystrokes(GO_BACK_KEY);
    cx.run_until_parked();
    assert_eq!(page_thread(&r, cx), Some(b));
    focus_ws(cx);
    cx.simulate_keystrokes(GO_FORWARD_KEY);
    cx.run_until_parked();
    assert_eq!(page_thread(&r, cx), Some(a));
    for (key, theirs) in [(GO_BACK_KEY, "shift-["), (GO_FORWARD_KEY, "shift-]")] {
        assert_ne!(key, format!("{M}-{theirs}"), "no shared key");
    }
}
