//! The sidebar is rebuilt when what a row shows changes, not on every notify of an open thread (a keystroke, a
//! streamed delta); a closed thread's subscriptions go with it, and a one-shot wait drops itself once it fires.

use super::Workspace;
use super::thread_model::Item;
use super::threads_test::{
    fixture, open_workspace, project_thread, setup, type_and_send, wait_done,
};
use gpui::{Entity, TestAppContext, VisualTestContext};
use skyelab_agent::DeltaKind;

fn rebuilds(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> usize {
    ws.read_with(cx, |ws, cx| ws.sidebar.read(cx).rebuilds)
}

#[gpui::test]
fn keystrokes_and_deltas_dont_rebuild_the_sidebar(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    cx.run_until_parked();
    let before = rebuilds(&ws, cx);
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_keystrokes("a b c");
    cx.run_until_parked();
    thread.update(cx, |t, cx| {
        t.items.push(Item::Live {
            kind: DeltaKind::Text,
            text: "streamed".into(),
        });
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(rebuilds(&ws, cx), before, "SIDEBAR_REBUILT_ON_NOTIFY");
    thread.update(cx, |t, cx| {
        t.turn_started_ms = Some(1);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(rebuilds(&ws, cx) > before, "a row change rebuilds it");
}

#[gpui::test]
fn closing_a_thread_drops_its_subscriptions(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    let key = thread.entity_id();
    ws.update(cx, |ws, cx| ws.wait_on(&thread, cx, |_, _, _| true));
    assert_eq!(ws.read_with(cx, |ws, _| ws.waiters.len()), 1);
    thread.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.waiters.len()),
        0,
        "WAITER_KEPT after it fired"
    );
    assert!(ws.read_with(cx, |ws, _| ws.thread_subs.contains_key(&key)));
    ws.update_in(cx, |ws, window, cx| ws.close_thread(&thread, window, cx));
    assert!(
        !ws.read_with(cx, |ws, _| ws.thread_subs.contains_key(&key)),
        "SUBS_KEPT after close"
    );
}
