//! Stage 5a acceptance: the terminal tab bar's "…" menu is our `ui::PopupMenu`, with the drawer's own controls
//! before the dock's Zoom item, and its items still drive the terminal.

use super::NewTerminal;
use super::worktree_thread_test::{fixture, open_thread, wait_until};
use gpui::{Modifiers, Role, VisualTestContext};
use gpui_base::test_support::snapshots;

fn menu_items(cx: &mut VisualTestContext) -> Vec<String> {
    let snaps = cx.update(|window, _| snapshots(window));
    let menu = snaps
        .iter()
        .find(|s| s.role() == Some(Role::Menu) && s.visible())
        .expect("our menu (role Menu) is open")
        .bounds();
    snaps
        .iter()
        .filter(|s| s.role() == Some(Role::MenuItem) && s.visible())
        .filter(|s| menu.contains(&s.bounds().center()))
        .filter_map(|s| s.label().map(str::to_owned))
        .collect()
}

#[gpui::test]
fn the_terminal_menu_lists_its_controls_and_splits(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread(&f, cx);
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "a terminal", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 1)
    });
    cx.run_until_parked();
    // The right dock can draw a "…" button too: the terminal's is the lowest one.
    let at = cx
        .update(|window, _| snapshots(window))
        .into_iter()
        .filter(|s| s.role() == Some(Role::Button) && s.label() == Some("More actions") && s.visible())
        .max_by(|a, b| a.bounds().origin.y.partial_cmp(&b.bounds().origin.y).unwrap_or(std::cmp::Ordering::Equal))
        .expect("the terminal tab bar's \"…\" button, named More actions")
        .bounds()
        .center();
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        menu_items(cx),
        ["Split right", "Split down", "New terminal", "Close terminal", "Zoom In"]
    );
    let split = cx
        .update(|window, _| snapshots(window))
        .into_iter()
        .find(|s| s.label() == Some("Split right") && s.visible())
        .expect("Split right drawn")
        .bounds()
        .center();
    cx.simulate_click(split, Modifiers::none());
    let panel = ws.read_with(cx, |ws, _| ws.terminals[0].clone());
    wait_until(cx, "a second pane", |cx| {
        panel.read_with(cx, |p, _| p.panes.len() == 2)
    });
}
