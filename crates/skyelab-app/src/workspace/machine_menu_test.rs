//! The machine menu: it opens where the chip was clicked (the palette's Switch Machine opens it unanchored), the
//! second open closes it, a pick closes it, and it is drawn over the window while open.

use super::env_harness::*;
use super::env_picker::{ShowEnvironments, ToggleEnvironments};
use super::manage_harness::rig;
use super::*;
use gpui::{TestAppContext, point, px};
use skyelab_daemon::testing::NODE_NAME;

#[gpui::test]
fn the_chip_opens_the_menu_at_the_click_and_a_pick_closes_it(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    let picker = ws.read_with(cx, |ws, _| ws.env_picker().expect("picker"));
    let open = |cx: &mut gpui::VisualTestContext| picker.read_with(cx, |p, _| (p.open, p.anchor));

    cx.dispatch_action(ShowEnvironments { x: 120., y: 540. });
    cx.run_until_parked();
    assert_eq!(open(cx), (true, Some(point(px(120.), px(540.)))));
    assert!(cx.debug_bounds("env-menu").is_some(), "the menu is drawn");

    cx.dispatch_action(ShowEnvironments { x: 120., y: 540. });
    cx.run_until_parked();
    assert!(!open(cx).0, "a second click on the chip closes it");
    assert!(cx.debug_bounds("env-menu").is_none());

    // The palette's Switch Machine: no chip, so no anchor.
    cx.dispatch_action(ToggleEnvironments);
    cx.run_until_parked();
    assert_eq!(open(cx), (true, None));
    assert!(cx.debug_bounds("env-menu").is_some());

    // Settings closes it, and it stays shut under the modal.
    cx.dispatch_action(OpenSettingsSection {
        section: crate::settings::Section::Machines,
    });
    cx.run_until_parked();
    assert!(!open(cx).0, "opening Settings closes the menu");
    cx.dispatch_action(ToggleEnvironments);
    cx.run_until_parked();
    assert!(
        !open(cx).0,
        "the menu doesn't open under the Settings modal"
    );
    ws.update_in(cx, |ws, window, cx| ws.close_settings(window, cx));
    cx.dispatch_action(ToggleEnvironments);
    cx.run_until_parked();
    assert!(open(cx).0);

    switch(&ws, Some(NODE_NAME), cx);
    cx.run_until_parked();
    assert!(!open(cx).0, "picking a machine closes the menu");
    picker.read_with(cx, |p, _| {
        assert_eq!(p.current.as_deref(), Some(NODE_NAME));
        assert!(
            p.remote.is_some(),
            "the row's status dot follows the connection"
        );
    });
}

const M: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};

#[gpui::test]
fn escape_closes_the_machine_menu_over_an_open_thread(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    r.ws.update_in(cx, |ws, window, cx| {
        ws.set_environments(
            Arc::new(FakeTailnet::default()),
            None,
            skyelab_client::Options::default(),
            window,
            cx,
        )
    });
    r.thread("a", cx);
    cx.simulate_keystrokes(&format!("{M}-1"));
    cx.run_until_parked();
    let picker = r.ws.read_with(cx, |ws, _| ws.env_picker().expect("picker"));

    cx.dispatch_action(ShowEnvironments { x: 120., y: 540. });
    cx.run_until_parked();
    assert!(
        picker.read_with(cx, |p, _| p.open),
        "precondition: the chip opened the menu"
    );

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        !picker.read_with(cx, |p, _| p.open),
        "MENU STILL OPEN after Escape"
    );
}
