//! Acceptance 1: with a thread waiting on the user, the quit dialog offers "Keep running in background"; picking it
//! closes every window without quitting, a hidden hub takes the thread, and its turn still finishes and is stored.

use super::manage_harness::{Rig, rig};
use super::quit_guard::{TestQuits, blockers, can_keep, request_quit};
use super::thread::ThreadView;
use super::thread_model::Item;
use super::threads_test::wait_until;
use super::windows::{self, window_handles};
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::Decision;

/// A thread in the rig's project whose agent asks to run a command and waits for the answer.
pub(crate) fn waiting_thread(cx: &mut VisualTestContext) -> Entity<ThreadView> {
    cx.update(|_, cx| cx.set_global(TestQuits(0)));
    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    let view = cx
        .update(|_, cx| cx.try_global::<windows::Windows>().map(|w| w.threads.clone()))
        .and_then(|t| t.last().cloned())
        .expect("a thread view");
    view.update_in(cx, |v, window, cx| {
        v.mode = skyelab_agent::Mode::Default; // a new thread's Full access would skip the ask
        v.submit_remote("permit", window, cx)
    })
    .expect("sent");
    wait_until(cx, "the agent to wait on an approval", |cx| {
        cx.update(|_, cx| blockers(cx).iter().any(|(_, b)| b.is_work()))
    });
    view
}

/// Allows the waiting thread's request.
pub(crate) fn approve(view: &Entity<ThreadView>, cx: &mut VisualTestContext) {
    let id = view.read_with(cx, |v, _| {
        v.items.iter().find_map(|i| match i {
            Item::Approval(a) if a.decision.is_none() => Some(a.id.clone()),
            _ => None,
        })
    });
    let id = id.expect("a request waiting");
    view.update(cx, |v, cx| v.answer(&id, Decision::AllowOnce, cx));
}

pub(crate) fn quits(cx: &mut VisualTestContext) -> usize {
    TestAppContext::update(cx, |cx| cx.try_global::<TestQuits>().map_or(0, |q| q.0))
}

/// The hidden hub's window, once there is one.
pub(crate) fn hidden_hub(cx: &mut VisualTestContext) -> Option<gpui::AnyWindowHandle> {
    TestAppContext::update(cx, |cx| {
        cx.windows()
            .into_iter()
            .find(|w| windows::is_hidden(*w, cx))
    })
}

#[gpui::test]
fn keep_running_closes_the_windows_and_the_turn_finishes(cx: &mut TestAppContext) {
    let (r, vcx): (Rig, _) = rig(cx);
    let view = waiting_thread(vcx);
    assert!(
        vcx.update(|_, cx| can_keep(&blockers(cx))),
        "a waiting thread: the dialog offers Keep running"
    );
    TestAppContext::update(vcx, request_quit);
    vcx.run_until_parked();
    let shown = vcx.debug_bounds("quit-keep").expect("the Keep button is drawn");
    vcx.simulate_click(shown.center(), gpui::Modifiers::none());
    vcx.run_until_parked();

    assert_eq!(quits(vcx), 0, "Keep running doesn't quit");
    assert!(
        TestAppContext::update(vcx, |cx| window_handles(cx).is_empty()),
        "every window closed"
    );
    assert!(TestAppContext::update(vcx, |cx| background::is_on(cx)), "background mode");
    let hub = hidden_hub(vcx).expect("a hidden hub took over");
    let mut hcx = VisualTestContext::from_window(hub, cx);
    let held = hcx.update(|_, cx| {
        cx.try_global::<windows::Windows>()
            .is_some_and(|w| w.threads.contains(&view))
    });
    assert!(held, "the thread view lives on");
    assert!(
        view.read_with(&hcx, |v, _| v.window == hub),
        "the hub owns the thread"
    );

    approve(&view, &mut hcx);
    let id = view.read_with(&hcx, |v, _| v.thread.as_ref().map(|t| t.id)).expect("row");
    wait_until(&mut hcx, "the turn to finish and be stored", |_| {
        r.row(id).is_some_and(|t| t.status != "running" && t.status != "starting")
    });
    assert_eq!(quits(&mut hcx), 0, "still running");
}
