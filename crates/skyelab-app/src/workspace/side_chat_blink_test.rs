//! The side chat's composer caret blink builds no timeline row, like the center thread's (docs/gpui/redraw.md).

use crate::workspace::thread::composer::Submit;
use crate::workspace::worktree_thread_test::{fixture, open_thread, send, wait_until};
use std::time::Duration;

#[gpui::test]
fn the_side_chats_caret_blink_builds_no_timeline_row(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    send(cx);
    wait_until(cx, "the parent's turn ends", |cx| {
        thread.read_with(cx, |t, _| t.thread.is_some() && !t.turn_running())
    });
    thread.update_in(cx, |t, window, cx| {
        t.prompt
            .update(cx, |p, cx| p.set_value("/side why is it slow?", window, cx));
        t.submit(Submit::Send, window, cx);
    });
    wait_until(cx, "the side chat pane", |cx| {
        ws.read_with(cx, |ws, _| ws.side_chats.len() == 1)
    });
    let side = ws.read_with(cx, |ws, cx| ws.side_chats[0].read(cx).view.clone());
    wait_until(cx, "the side chat's answer", |cx| {
        side.read_with(cx, |v, _| v.last_answer().is_some() && !v.turn_running())
    });
    cx.update(|window, _| window.activate_window());
    side.update_in(cx, |v, window, cx| v.focus_prompt(window, cx));
    cx.simulate_keystrokes("a");
    cx.executor().advance_clock(Duration::from_millis(1500));
    cx.run_until_parked();
    crate::debug_renders::take_test_count("timeline row");
    crate::debug_renders::take_test_count("ThreadView");
    for _ in 0..6 {
        cx.executor().advance_clock(Duration::from_millis(500));
        cx.run_until_parked();
    }
    let views = crate::debug_renders::take_test_count("ThreadView");
    assert!(
        views >= 4,
        "the blink redraws the side chat's composer, got {views}"
    );
    assert_eq!(
        crate::debug_renders::take_test_count("timeline row"),
        0,
        "SIDE_CHAT_BLINK_REDRAWS_TIMELINE"
    );
}
