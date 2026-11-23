//! F6 in a real workspace: the order, hidden areas skipped, the ring, and Esc back to the message input.

use super::focus_areas::{Area, FocusRing};
use super::manage_harness::rig;
use gpui::{TestAppContext, VisualTestContext};

const M: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};

fn at(r: &super::manage_harness::Rig, cx: &mut VisualTestContext) -> Option<Area> {
    r.ws.update_in(cx, |ws, window, cx| {
        let areas = ws.areas(window, cx);
        areas
            .into_iter()
            .find(|(.., inside)| *inside)
            .map(|(a, ..)| a)
    })
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

fn ringed(cx: &mut VisualTestContext) -> Option<Area> {
    cx.update(|_, cx| {
        cx.try_global::<FocusRing>()
            .and_then(|r| r.0.as_ref().map(|(a, _)| *a))
    })
}

#[gpui::test]
fn f6_cycles_the_shown_areas_and_esc_returns_to_the_composer(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    r.thread("a", cx);
    press(&format!("{M}-1"), cx);
    assert_eq!(
        at(&r, cx),
        Some(Area::Composer),
        "opening a thread focuses its input"
    );

    // Right and bottom docks start closed: sidebar, conversation, input.
    press("f6", cx);
    assert_eq!(
        at(&r, cx),
        Some(Area::Sidebar),
        "wraps from the input to the sidebar"
    );
    assert_eq!(ringed(cx), Some(Area::Sidebar));
    press("f6", cx);
    assert_eq!(at(&r, cx), Some(Area::Timeline));
    press("f6", cx);
    assert_eq!(at(&r, cx), Some(Area::Composer));
    press("shift-f6", cx);
    assert_eq!(at(&r, cx), Some(Area::Timeline));

    press("escape", cx);
    assert_eq!(
        at(&r, cx),
        Some(Area::Composer),
        "Esc goes back to the input"
    );

    // A shown right panel joins the cycle; a hidden sidebar leaves it.
    r.ws.update_in(cx, |ws, window, cx| {
        ws.toggle_right(&super::layout::ToggleRightPanel, window, cx);
        ws.toggle_sidebar(&super::ToggleSidebar, window, cx);
    });
    cx.run_until_parked();
    r.ws.update_in(cx, |ws, window, cx| {
        ws.current_thread()
            .expect("thread shown")
            .update(cx, |v, cx| v.focus_prompt(window, cx))
    });
    press("f6", cx);
    assert_eq!(at(&r, cx), Some(Area::Right));
    press("f6", cx);
    assert_eq!(
        at(&r, cx),
        Some(Area::Timeline),
        "no sidebar: wraps to the conversation"
    );
    press("escape", cx);
    assert_eq!(at(&r, cx), Some(Area::Composer));

    // A click ends keyboard navigation.
    assert!(ringed(cx).is_some());
    cx.simulate_mouse_down(
        gpui::point(gpui::px(5.), gpui::px(5.)),
        gpui::MouseButton::Left,
        gpui::Modifiers::none(),
    );
    assert_eq!(ringed(cx), None);
}
