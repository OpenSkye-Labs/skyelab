//! Acceptance (QA #14): the terminal drawer has T3's controls. Split right, Split down and New terminal buttons sit
//! in its tab bar and act on the shown tab even when focus is elsewhere; Toggle terminal with none open starts a
//! shell; two tabs in one folder get different titles.

use super::worktree_thread_test::{fixture, open_thread, wait_until};
use super::*;
use gpui::VisualTestContext;
use gpui_component::dock::DockPlacement;

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let at = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(at.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

fn terminals(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> usize {
    ws.read_with(cx, |ws, _| ws.terminals.len())
}

#[gpui::test]
fn the_tab_bar_buttons_split_and_add_terminals(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread(&f, cx);
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "a terminal", |cx| terminals(&ws, cx) == 1);
    let panel = ws.read_with(cx, |ws, _| ws.terminals[0].clone());
    // Focus leaves the terminal: the buttons still act on the tab they sit on.
    ws.update_in(cx, |ws, window, cx| ws.focus.focus(window, cx));

    click(cx, "terminal-split-right");
    wait_until(cx, "a pane beside", |cx| {
        panel.read_with(cx, |p, _| p.panes.len() == 2)
    });
    click(cx, "terminal-split-down");
    wait_until(cx, "a pane under", |cx| {
        panel.read_with(cx, |p, _| p.panes.len() == 3)
    });
    assert_eq!(terminals(&ws, cx), 1, "a split is not a new tab");
    click(cx, "terminal-new");
    wait_until(cx, "a second tab", |cx| terminals(&ws, cx) == 2);
}

#[gpui::test]
fn toggle_terminal_with_none_open_starts_a_shell(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread(&f, cx);
    let open = |cx: &mut VisualTestContext| {
        ws.read_with(cx, |ws, cx| ws.dock.read(cx).is_dock_open(DockPlacement::Bottom))
    };
    if open(cx) {
        cx.dispatch_action(ToggleBottomPanel);
        cx.run_until_parked();
    }
    assert_eq!(terminals(&ws, cx), 0, "closing starts nothing");
    cx.dispatch_action(ToggleBottomPanel);
    wait_until(cx, "a shell", |cx| terminals(&ws, cx) == 1);
    assert!(open(cx));
}

#[gpui::test]
fn two_tabs_in_one_folder_get_different_titles(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread(&f, cx);
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "a terminal", |cx| terminals(&ws, cx) == 1);
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "two terminals", |cx| terminals(&ws, cx) == 2);
    let labels: Vec<_> = ws.read_with(cx, |ws, cx| {
        ws.terminals.iter().map(|t| t.read(cx).tab_label()).collect()
    });
    assert_ne!(labels[0], labels[1], "tabs are told apart: {labels:?}");
}
