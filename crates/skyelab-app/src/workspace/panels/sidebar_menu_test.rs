//! QA: right-clicking a sidebar thread row opens its menu (Rename, Regenerate title, Pin, Archive, Delete), and the
//! menu's entries act on that thread.

use super::manage_harness::rig;
use gpui::{Modifiers, MouseButton, TestAppContext, VisualTestContext};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.draw(cx).clear(cx);
    });
}

#[gpui::test]
fn right_click_opens_the_thread_menu_and_pin_pins(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let id = r.thread("Fix it", cx);
    draw(cx);
    let selector: &'static str = format!("row-thread-{id}").leak();
    let at = cx
        .debug_bounds(selector)
        .expect("the row is drawn")
        .center();
    let before = cx.update(|window, cx| window.focused(cx));
    cx.simulate_mouse_move(at, None, Modifiers::default());
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
    draw(cx);
    let after = cx.update(|window, cx| window.focused(cx));
    assert_ne!(before, after, "the menu takes the focus when it opens");
    let sidebar = r.ws.read_with(cx, |ws, _| ws.sidebar.clone());
    assert!(
        sidebar.read_with(cx, |s, _| s.snooze.open.is_some()),
        "MENU_NOT_DRAWN_BY_THE_SIDEBAR"
    );

    // Down to Pin (the third entry) and Enter.
    cx.simulate_keystrokes("down down down enter");
    draw(cx);
    assert!(
        r.row(id).expect("row").pinned_at.is_some(),
        "MENU_PIN_DIDNT_PIN"
    );
}
