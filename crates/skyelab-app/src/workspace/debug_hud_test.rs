//! Debug mode off adds no redraws, the overlay shows and ticks once a second, and its actions are in the palette.

use super::super::palette_test::open;
use super::super::threads_test::test_backend;
use super::*;
use gpui::TestAppContext;

#[gpui::test]
fn debug_off_adds_no_redraws_and_the_overlay_ticks_while_shown(cx: &mut TestAppContext) {
    super::super::palette_test::setup(cx);
    let (ws, cx) = open(test_backend(), cx);
    // Let startup work and the caret's idle timeout settle, then watch an idle window.
    cx.executor().advance_clock(Duration::from_secs(30));
    cx.run_until_parked();
    crate::debug_renders::take_test_count("Workspace");
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
    assert!(
        ws.read_with(cx, |ws, _| ws.debug_hud.is_none()),
        "no overlay entity while it is off"
    );
    assert_eq!(
        crate::debug_renders::take_test_count("Workspace"),
        0,
        "an idle window without the overlay draws nothing"
    );

    cx.dispatch_action(ToggleDebugOverlay);
    cx.run_until_parked();
    assert!(
        skyelab_core::debug::enabled(),
        "showing the overlay turns debug mode on"
    );
    assert!(
        cx.debug_bounds("debug-hud").is_some(),
        "the overlay is drawn"
    );
    crate::debug_renders::take_test_count("DebugHud");
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
    let ticks = crate::debug_renders::take_test_count("DebugHud");
    assert!(
        (4..=6).contains(&ticks),
        "about one redraw a second, got {ticks}"
    );

    cx.dispatch_action(ToggleDebugOverlay);
    cx.run_until_parked();
    assert!(
        ws.read_with(cx, |ws, _| ws.debug_hud.is_none()),
        "hidden again"
    );
    crate::debug_renders::take_test_count("Workspace");
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
    assert_eq!(
        crate::debug_renders::take_test_count("DebugHud"),
        0,
        "its tick ends with it"
    );
}

#[gpui::test]
fn the_debug_actions_are_in_the_palette(cx: &mut TestAppContext) {
    super::super::palette_test::setup(cx);
    let (ws, cx) = open(test_backend(), cx);
    let labels: Vec<String> = ws.update_in(cx, |ws, window, cx| {
        let focus = ws.focus.clone();
        ws.palette_entries(&focus, window, cx)
            .into_iter()
            .map(|e| e.label.to_string())
            .collect()
    });
    for label in ["Toggle Debug Overlay", "Open Debug Log", "Dump Debug State"] {
        assert!(
            labels.contains(&label.to_owned()),
            "{label} missing: {labels:?}"
        );
    }
}
