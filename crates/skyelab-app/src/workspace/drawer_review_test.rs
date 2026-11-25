//! Review fixes for the drawer/dock change: Esc in Devices' Open URL field closes only that row, hiding a dock ends
//! a zoom in it, Esc on "Open a surface" hides the right panel, the Close button closes the active pane, and a tab
//! is numbered only against unnamed tabs in its folder.

use super::devices_tests::{Fake, open};
use super::worktree_thread_test::{fixture, open_thread, wait_until};
use super::*;
use gpui::VisualTestContext;
use gpui_component::dock::{DockPlacement, ToggleZoom};
use std::sync::Arc;

fn right_open(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> bool {
    ws.read_with(cx, |ws, cx| ws.dock.read(cx).is_dock_open(DockPlacement::Right))
}

#[gpui::test]
fn escape_in_the_url_field_closes_only_that_row(cx: &mut gpui::TestAppContext) {
    let (ws, panel, cx) = open(cx, Arc::<Fake>::default());
    cx.run_until_parked();
    panel.update_in(cx, |p, window, cx| p.show_url_row("emulator-5554", window, cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(right_open(&ws, cx), "Esc in the field keeps Devices");
    assert!(panel.read_with(cx, |p, _| p.url_for.is_none()), "and closes the row");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!right_open(&ws, cx), "the next Esc hides the tab");
}

#[gpui::test]
fn hiding_a_zoomed_tab_ends_the_zoom(cx: &mut gpui::TestAppContext) {
    let (ws, panel, cx) = open(cx, Arc::<Fake>::default());
    panel.update_in(cx, |p, window, cx| p.focus_handle(cx).focus(window, cx));
    cx.run_until_parked();
    cx.dispatch_action(ToggleZoom);
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, cx| ws.dock.read(cx).is_zoomed()));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!right_open(&ws, cx));
    assert!(
        !ws.read_with(cx, |ws, cx| ws.dock.read(cx).is_zoomed()),
        "no zoom stuck in a closed dock"
    );
}

#[gpui::test]
fn escape_on_open_a_surface_hides_the_right_panel(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread(&f, cx);
    if !right_open(&ws, cx) {
        cx.dispatch_action(super::layout::ToggleRightPanel);
        cx.run_until_parked();
    }
    assert!(right_open(&ws, cx));
    ws.update_in(cx, |ws, window, cx| {
        ws.surfaces.read(cx).focus_handle(cx).focus(window, cx)
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!right_open(&ws, cx));
}

#[gpui::test]
fn the_close_button_closes_the_active_pane(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread(&f, cx);
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "a terminal", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 1)
    });
    cx.run_until_parked();
    let at = cx.debug_bounds("terminal-close").expect("the Close button is drawn");
    cx.simulate_click(at.center(), gpui::Modifiers::none());
    wait_until(cx, "the tab closed", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.is_empty())
    });
}

#[gpui::test]
fn a_lone_shell_is_not_numbered_after_a_named_tab(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, _t, cx) = open_thread(&f, cx);
    ws.update_in(cx, |ws, window, cx| {
        let (o, a) = ws.terminal_job(cx)().expect("options");
        ws.add_terminal_tab(o, a, Some(("Build".into(), String::new())), window, cx)
            .expect("named tab");
    });
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "the shell", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 2)
    });
    let label = ws.read_with(cx, |ws, cx| ws.terminals[1].read(cx).tab_label());
    assert!(!label.ends_with(" 2"), "the only unnamed shell reads {label:?}");
}
