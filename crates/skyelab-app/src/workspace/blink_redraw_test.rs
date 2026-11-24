//! The focused composer's caret blink redraws the thread view but builds no timeline row: the timeline is a cached
//! view and the dock draws the center uncached (docs/gpui/redraw.md).

use super::threads_test::{fixture, open_workspace, project_thread, setup, type_and_send, wait_done};
use gpui::TestAppContext;
use std::time::Duration;

#[gpui::test]
fn the_caret_blink_builds_no_timeline_row(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    // The caret blinks only in the active window.
    cx.update(|window, _| window.activate_window());
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_keystrokes("a");
    // The keystroke's own redraws (the draft's save) settle first: they redraw the thread, rows included.
    cx.executor().advance_clock(Duration::from_millis(1500));
    cx.run_until_parked();
    crate::debug_renders::take_test_count("timeline row");
    crate::debug_renders::take_test_count("ThreadView");
    for _ in 0..6 {
        cx.executor().advance_clock(Duration::from_millis(500));
        cx.run_until_parked();
    }
    let views = crate::debug_renders::take_test_count("ThreadView");
    assert!(views >= 4, "the blink redraws the composer, got {views}");
    assert_eq!(
        crate::debug_renders::take_test_count("timeline row"),
        0,
        "BLINK_REDRAWS_TIMELINE"
    );
    // Typing still shows in the composer.
    cx.simulate_keystrokes("b");
    cx.run_until_parked();
    let text = thread.read_with(cx, |t, cx| t.prompt.read(cx).value().to_string());
    assert!(text.ends_with("ab"), "typed text shows, got {text:?}");
}
