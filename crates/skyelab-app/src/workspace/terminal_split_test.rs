//! Acceptance: a split adds a pane beside the focused one in the same tab, under the folder's account, and the
//! focus keys cycle through the panes.

use super::NewTerminal;
use super::terminal_panel::{NEXT_TERMINAL_KEY, PREVIOUS_TERMINAL_KEY, SPLIT_TERMINAL_KEY};
use super::worktree_thread_test::{fixture, open_thread, wait_until};
use gpui::Focusable as _;

#[gpui::test]
fn split_adds_a_pane_and_focus_cycles(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread(&f, cx);
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "a terminal", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 1)
    });
    let panel = ws.read_with(cx, |ws, _| ws.terminals[0].clone());
    let first = panel.read_with(cx, |p, _| p.panes[0].view.clone());
    first.update_in(cx, |v, window, cx| v.focus_handle(cx).focus(window, cx));

    cx.simulate_keystrokes(SPLIT_TERMINAL_KEY);
    wait_until(cx, "a second pane", |cx| {
        panel.read_with(cx, |p, _| p.panes.len() == 2)
    });
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.terminals.len()),
        1,
        "a split is not a new tab"
    );
    let account = panel.read_with(cx, |p, _| p.panes[1].account.as_ref().map(|a| a.id.clone()));
    assert_eq!(
        account.as_deref(),
        Some("acme"),
        "the split runs under the folder's account"
    );
    let focused = |cx: &mut gpui::VisualTestContext| {
        panel.update_in(cx, |p, window, cx| p.focused_pane(window, cx))
    };
    assert_eq!(focused(cx), Some(1), "the new pane takes focus");

    cx.simulate_keystrokes(NEXT_TERMINAL_KEY);
    assert_eq!(focused(cx), Some(0), "next wraps around");
    cx.simulate_keystrokes(PREVIOUS_TERMINAL_KEY);
    assert_eq!(focused(cx), Some(1));
}

/// Dragging the divider between two panes moves it under the mouse; the tab keeps the new sizes.
#[gpui::test]
fn dragging_the_divider_resizes_the_panes(cx: &mut gpui::TestAppContext) {
    use gpui::{Modifiers, MouseButton, point, px};
    use skyelab_core::terminal_layout::PaneTree;
    let f = fixture();
    let (ws, _thread, cx) = open_thread(&f, cx);
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "a terminal", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 1)
    });
    let panel = ws.read_with(cx, |ws, _| ws.terminals[0].clone());
    let first = panel.read_with(cx, |p, _| p.panes[0].view.clone());
    first.update_in(cx, |v, window, cx| v.focus_handle(cx).focus(window, cx));
    cx.simulate_keystrokes(SPLIT_TERMINAL_KEY);
    wait_until(cx, "a second pane", |cx| {
        panel.read_with(cx, |p, _| p.panes.len() == 2)
    });
    cx.run_until_parked();
    let divider = cx
        .debug_bounds("terminal-divider-0")
        .expect("the divider is drawn");
    let start = divider.center();
    let to = point(start.x - px(100.), start.y);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        point(start.x - px(20.), start.y),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    let PaneTree::Split { sizes, .. } = panel.read_with(cx, |p, _| p.tree.clone()) else {
        panic!("two panes split");
    };
    assert!(sizes[0] < 5_000, "the left pane shrank: {sizes:?}");
    assert_eq!(sizes.iter().sum::<u32>(), 10_000);
    let moved = cx.debug_bounds("terminal-divider-0").expect("still drawn");
    assert!(
        (moved.center().x - to.x).abs() < px(8.),
        "the divider follows the mouse: {:?} vs {:?}",
        moved.center(),
        to
    );
}
