//! The Details tab: the right-panel toggle shows it, it lists the threads the shown thread's agent delegated with
//! their live status, and every account's newest usage reading.
#![cfg(unix)]

use super::delegation_test::{open, rig, send, start, wait_until};
use super::layout::ToggleRightPanel;
use super::thread::Stop;
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
    wait_until(cx, "the work account's usage", |cx| {
        ws.read_with(cx, |ws, cx| {
            let usage = &ws.details.read(cx).usage;
            usage.iter().any(|a| a.id == "work" && a.windows.len() == 2)
        })
    });
    ws.read_with(cx, |ws, cx| {
        let usage = &ws.details.read(cx).usage;
        let work = usage.iter().find(|a| a.id == "work").expect("work listed");
        assert_eq!(work.windows, vec![("Session", 0.91), ("Weekly", 0.62)]);
        assert!(work.as_of_ms.is_some());
        // Every configured account is listed; one whose agents reported nothing has no numbers.
        assert!(
            usage
                .iter()
                .any(|a| a.id != "work" && a.windows.is_empty() && a.as_of_ms.is_none())
        );
    });

    assert!(
        cx.debug_bounds("thread-details-card").is_none(),
        "the dock starts closed"
    );
    cx.dispatch_action(ToggleRightPanel);
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("thread-details-card").is_some(),
        "the right-panel toggle shows the Details tab"
    );

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
