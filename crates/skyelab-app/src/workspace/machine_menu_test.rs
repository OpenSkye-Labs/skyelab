//! The machine menu: it opens where the chip was clicked (the palette's Switch Machine opens it unanchored), the
//! second open closes it, a pick closes it, and it is drawn over the window while open.

use super::env_harness::*;
use super::env_picker::{ShowEnvironments, ToggleEnvironments};
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
