//! The Details tab: the right-panel toggle shows it, it lists the threads the shown thread's agent delegated with
//! their live status, and every account's newest usage reading.
#![cfg(unix)]

use crate::ui::Status;
use crate::workspace::delegation_test::{open, rig, send, start, wait_until};
use crate::workspace::layout::ToggleRightPanel;
use crate::workspace::thread::Stop;
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
    // A limit reported after the turn's result still reaches the tab, its percentages kept.
    send(&parent, "limit", cx);
    wait_until(cx, "the work account shown limited", |cx| {
        ws.read_with(cx, |ws, cx| {
            let usage = &ws.details.read(cx).usage;
            usage
                .iter()
                .any(|a| a.id == "work" && matches!(a.limited, Some(Some(_))))
        })
    });
    ws.read_with(cx, |ws, cx| {
        let usage = &ws.details.read(cx).usage;
        let work = usage.iter().find(|a| a.id == "work").expect("work listed");
        let windows: Vec<(&str, f32)> = work.windows.iter().map(|w| (w.label, w.used)).collect();
        assert_eq!(windows, vec![("5h", 0.91), ("Weekly", 0.62)]);
        assert!(
            work.windows.iter().all(|w| w.at_ms > 0),
            "each bar has its reading's time"
        );
        // Every configured account is listed; one whose agents reported nothing has no numbers.
        assert!(
            usage
                .iter()
                .any(|a| a.id != "work" && a.windows.is_empty() && a.limited.is_none())
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

    // Link pull request…: only a GitHub PR link is kept, normalized, and the sidebar row shows its number.
    parent.update_in(cx, |p, window, cx| {
        p.open_link_pr(window, cx);
        let input = p.details.pr_input.clone().expect("the link field");
        input.update(cx, |s, cx| {
            s.set_value("https://gitlab.com/o/r/pull/7", window, cx)
        });
        p.link_pr(window, cx);
        assert!(p.details.pr_error.is_some(), "a non-GitHub link is refused");
        input.update(cx, |s, cx| {
            s.set_value("https://github.com/acme/demo/pull/42/files", window, cx)
        });
        p.link_pr(window, cx);
    });
    cx.run_until_parked();
    let (id, pr) = parent.read_with(cx, |p, _| {
        let t = p.thread.as_ref().expect("row");
        (t.id, t.pr_url.clone())
    });
    assert_eq!(pr.as_deref(), Some("https://github.com/acme/demo/pull/42"));
    let stored = ws.read_with(cx, |ws, _| {
        super::super::lock(&ws.backend.store)
            .expect("store")
            .thread(id)
            .expect("read")
            .and_then(|t| t.pr_url)
    });
    assert_eq!(stored, pr, "kept on the thread's row");
    let badge = ws.read_with(cx, |ws, cx| {
        ws.sidebar
            .read(cx)
            .marks
            .get(&gpui::SharedString::from(format!("thread-{id}")))
            .and_then(|m| m.pr.clone())
    });
    assert_eq!(badge.as_deref(), Some("#42"), "the sidebar badge");

    // The Commit & push menu runs in the thread's Changes tab; with no message typed, Commit points at the field.
    parent.update(cx, |_, cx| {
        cx.emit(crate::workspace::thread::ThreadViewEvent::Git(
            crate::workspace::thread::GitAction::Commit,
        ))
    });
    cx.run_until_parked();
    let refused = ws.read_with(cx, |ws, cx| {
        ws.changes_views
            .iter()
            .filter_map(|c| c.upgrade())
            .find_map(|c| c.read(cx).publish_error.clone())
    });
    assert_eq!(
        refused.as_deref(),
        Some("Write a commit message, then commit.")
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
