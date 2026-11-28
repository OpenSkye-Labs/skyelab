//! The details card's link field: right-click, then a click on Paste, pastes and leaves the card open. The menu hangs
//! below the short card, and a press on it is not a press outside the card.
#![cfg(unix)]

use crate::workspace::layout::ToggleThreadDetails;
use crate::workspace::threads_test::{fixture, open_workspace, project_thread, setup};
use gpui::{
    ClipboardItem, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px, size,
};
use gpui_base::test_support::snapshots;

const URL: &str = "https://github.com/acme/demo/pull/42";

fn bounds(cx: &mut VisualTestContext, label: &str) -> gpui::Bounds<gpui::Pixels> {
    cx.update(|window, _| snapshots(window))
        .into_iter()
        .find(|s| s.label() == Some(label))
        .unwrap_or_else(|| panic!("{label} drawn"))
        .bounds()
}

#[gpui::test]
fn a_click_on_the_field_menus_paste_is_not_outside_the_card(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    cx.simulate_resize(size(px(1400.), px(1000.)));
    cx.dispatch_action(ToggleThreadDetails);
    cx.run_until_parked();
    assert!(
        ws.update_in(cx, |ws, window, cx| ws.card_covers_chat(window, cx)),
        "setup: the card covers the chat"
    );
    thread.update_in(cx, |t, window, cx| t.open_link_pr(window, cx));
    cx.run_until_parked();
    let input = thread.read_with(cx, |t, _| t.details.pr_input.clone().expect("setup: field"));
    cx.write_to_clipboard(ClipboardItem::new_string(URL.into()));

    let field = bounds(cx, "Pull request link");
    let at = point(field.left() + px(10.), field.center().y);
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();

    let paste = bounds(cx, "Paste").center();
    cx.simulate_mouse_move(paste, None, Modifiers::none());
    cx.simulate_click(paste, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        input.read_with(cx, |i, _| i.value().to_string()),
        URL,
        "PASTE_LOST: the click on Paste did not paste"
    );
    assert!(
        cx.debug_bounds("thread-details-card").is_some(),
        "CARD_CLOSED: a press on the field's own menu closed the card"
    );
}
