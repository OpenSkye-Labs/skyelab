//! The thread details card: it opens over the thread page, lists the threads its agent delegated with their live
//! status, and shows the usage windows the bound account's agent reported.
#![cfg(unix)]

use super::delegation_test::{open, rig, send, start, wait_until};
use super::thread::Stop;
use super::thread_details::ToggleThreadDetails;
use crate::ui::Status;
use gpui::TestAppContext;

#[gpui::test]
fn the_card_lists_children_by_status_and_the_reported_usage(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let parent = start(
        &ws,
        rig.work_a,
        &format!("delegate {} hold", rig.work_b),
        cx,
    );
    // The child's own view drives its link: starting, then running while its turn holds.
    wait_until(cx, "the child listed as running", |cx| {
        parent.read_with(
            cx,
            |p, _| matches!(&p.children[..], [c] if c.status == Status::Running),
        )
    });

    wait_until(cx, "the parent's turn done", |cx| {
        parent.read_with(cx, |p, _| p.live_status() == Status::Idle)
    });
    send(&parent, "usage", cx);
    wait_until(cx, "both usage windows", |cx| {
        parent.read_with(cx, |p, _| p.details.limits.len() == 2)
    });
    parent.read_with(cx, |p, _| {
        assert!(p.details.limits.contains(&("Weekly", 0.62)));
        assert!(p.details.limits.contains(&("Session", 0.91)));
    });

    assert!(cx.debug_bounds("thread-details-card").is_none());
    cx.dispatch_action(ToggleThreadDetails);
    cx.run_until_parked();
    assert!(parent.read_with(cx, |p, _| p.details.open));
    assert!(
        cx.debug_bounds("thread-details-card").is_some(),
        "the card draws over the thread page"
    );
    cx.dispatch_action(ToggleThreadDetails);
    cx.run_until_parked();
    assert!(cx.debug_bounds("thread-details-card").is_none());

    // Stopped, the child moves to Previous agents.
    let child_id = parent.read_with(cx, |p, _| p.children[0].id);
    let child = ws
        .read_with(cx, |ws, cx| ws.open_view(child_id, cx))
        .expect("the child's view is open");
    child.update_in(cx, |v, window, cx| v.stop(&Stop, window, cx));
    wait_until(cx, "the child listed as stopped", |cx| {
        parent.read_with(cx, |p, _| p.children[0].status == Status::Stopped)
    });
    parent.update_in(cx, |p, window, cx| p.stop(&Stop, window, cx));
    cx.run_until_parked();
}
