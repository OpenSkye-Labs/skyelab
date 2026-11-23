//! The Working shelf follows each thread's session state: a thread whose stored row says it is running is there with
//! no view open, and one waiting on an approval is there after its turn ended.

use super::manage_harness::{Rig, rig};
use super::threads_test::wait_until;
use super::*;
use gpui::{TestAppContext, VisualTestContext};

/// The titles in the sidebar section named `title`.
fn section(r: &Rig, title: &str, cx: &mut VisualTestContext) -> Vec<String> {
    let sidebar = r.ws.read_with(cx, |ws, _| ws.sidebar.clone());
    sidebar.read_with(cx, |s, _| {
        s.sections
            .iter()
            .filter(|s| s.title.as_ref() == title)
            .flat_map(|s| s.items.iter().map(|i| i.label.to_string()))
            .collect()
    })
}

#[gpui::test]
fn stored_running_thread_without_view_is_working(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let id = r.thread("busy elsewhere", cx);
    r.thread("quiet", cx);
    r.f.store
        .lock()
        .expect("lock")
        .set_thread_status(id, "running")
        .expect("status");
    r.reload(cx);
    assert!(
        r.ws.read_with(cx, |ws, cx| ws.open_view(id, cx).is_none()),
        "no view is open for it"
    );
    assert_eq!(section(&r, "Working", cx), vec!["busy elsewhere"]);
}

#[gpui::test]
fn approval_after_turn_is_working(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let view = new_thread(&r, cx);
    view.update_in(cx, |v, window, cx| v.submit_remote("timeline", window, cx))
        .expect("sent");
    wait_until(cx, "the turn ended with a question left waiting", |cx| {
        view.read_with(cx, |v, _| {
            !v.turn_running()
                && v.items
                    .iter()
                    .any(|i| matches!(i, thread_model::Item::Approval(a) if a.decision.is_none()))
        })
    });
    cx.run_until_parked();
    let title = view.read_with(cx, |v, _| v.title().to_string());
    assert_eq!(section(&r, "Working", cx), vec![title]);
}

/// A new thread tab in the rig's project; its row is stored with its first prompt.
pub(super) fn new_thread(r: &Rig, cx: &mut VisualTestContext) -> Entity<thread::ThreadView> {
    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    r.ws.read_with(cx, |ws, _| ws.open_threads.last().cloned())
        .expect("new thread tab")
}
