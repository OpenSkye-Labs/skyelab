//! A thread row's hover card opens only after the pointer rested on the row for the delay, as T3's does, and never
//! straight after a click: the click's row (or whichever row the list moves under the pointer) waits for the
//! pointer to move and rest again.

use super::manage_harness::rig;
use gpui::{Modifiers, TestAppContext, VisualTestContext, point, px};
use std::time::Duration;

fn settle(wait_ms: u64, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_millis(wait_ms));
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.draw(cx).clear(cx);
    });
}

#[gpui::test]
fn no_card_straight_after_a_click(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let id = r.thread("Fix it", cx);
    settle(0, cx);
    let selector: &'static str = format!("row-thread-{id}").leak();
    let at = cx
        .debug_bounds(selector)
        .expect("the row is drawn")
        .center();

    // Onto the row and click before the delay is up: the card must not open after it.
    cx.simulate_mouse_move(at, None, Modifiers::default());
    settle(100, cx);
    cx.simulate_click(at, Modifiers::default());
    settle(1000, cx);
    assert!(
        cx.debug_bounds("thread-card").is_none(),
        "no card straight after a click"
    );

    // The pointer moves and rests: now it opens, after the delay.
    cx.simulate_mouse_move(at + point(px(1.), px(0.)), None, Modifiers::default());
    settle(100, cx);
    assert!(
        cx.debug_bounds("thread-card").is_none(),
        "not before the delay"
    );
    settle(600, cx);
    assert!(
        cx.debug_bounds("thread-card").is_some(),
        "the card opens on a rest"
    );
}
