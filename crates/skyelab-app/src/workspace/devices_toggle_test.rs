//! Acceptance (QA #2): Devices is a real toggle. Its key (and the status-bar button, which sends the same action)
//! opens it, the same again while it shows hides it, and Esc inside it hides it.

use super::devices_tests::{Fake, open};
use super::keys::OPEN_DEVICES_KEY;
use super::*;
use crate::ui::dock::DockPlacement;
use std::sync::Arc;

fn right_open(ws: &Entity<Workspace>, cx: &mut gpui::VisualTestContext) -> bool {
    ws.read_with(cx, |ws, cx| ws.dock.read(cx).is_dock_open(DockPlacement::Right))
}

#[gpui::test]
fn a_second_press_hides_devices_and_a_third_shows_it(cx: &mut gpui::TestAppContext) {
    let (ws, _panel, cx) = open(cx, Arc::<Fake>::default());
    assert!(ws.read_with(cx, |ws, cx| ws.devices_shown(cx)), "the first press shows it");

    ws.update_in(cx, |ws, window, cx| ws.focus.focus(window, cx));
    cx.simulate_keystrokes(OPEN_DEVICES_KEY);
    cx.run_until_parked();
    assert!(!right_open(&ws, cx), "the second press hides the right panel");
    assert!(!ws.read_with(cx, |ws, cx| ws.devices_shown(cx)));

    cx.dispatch_action(OpenDevices); // the status-bar button's action
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, cx| ws.devices_shown(cx)), "a third press shows it again");
}

#[gpui::test]
fn escape_inside_devices_hides_it(cx: &mut gpui::TestAppContext) {
    let (ws, panel, cx) = open(cx, Arc::<Fake>::default());
    panel.update_in(cx, |p, window, cx| p.focus_handle(cx).focus(window, cx));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!right_open(&ws, cx), "Esc in the Devices tab hides it");
}
