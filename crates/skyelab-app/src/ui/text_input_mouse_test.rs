//! Own UI kit, stage 6a (.agent/plans/2026-11-29-own-ui-kit-text-editing-6a.md), criterion 7: mouse selection (click,
//! drag, shift-click, double and triple click) and the right-click menu, by simulated mouse events.

use super::text_input_harness::{events, focused, range_bounds, selection, value};
use crate::ui::TextInputEvent;
use gpui::{
    ClipboardItem, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point,
    TestAppContext, VisualTestContext, point,
};

const TEXT: &str = "alpha beta gamma";

/// A point a quarter (`frac` 0.25) or three quarters into the character at byte `i`, on the text's middle line.
fn at(
    view: &gpui::Entity<super::text_input_harness::Field>,
    vcx: &mut VisualTestContext,
    i: usize,
    frac: f32,
) -> Point<Pixels> {
    let b = range_bounds(view, vcx, i..i + 1);
    point(b.origin.x + b.size.width * frac, b.center().y)
}

fn press(
    vcx: &mut VisualTestContext,
    p: Point<Pixels>,
    button: MouseButton,
    modifiers: Modifiers,
    clicks: usize,
) {
    vcx.simulate_event(MouseDownEvent {
        button,
        position: p,
        modifiers,
        click_count: clicks,
        first_mouse: false,
    });
    vcx.simulate_event(MouseUpEvent {
        button,
        position: p,
        modifiers,
        click_count: clicks,
    });
}

#[gpui::test]
fn click_drag_and_shift_click(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 200., TEXT);
    let b = at(&view, vcx, 6, 0.25);
    press(vcx, b, MouseButton::Left, Modifiers::none(), 1);
    assert_eq!(
        selection(&view, vcx),
        6..6,
        "the left part of b puts the caret before it"
    );

    let end = at(&view, vcx, 15, 0.75);
    press(vcx, end, MouseButton::Left, Modifiers::shift(), 1);
    assert_eq!(
        selection(&view, vcx),
        6..16,
        "shift-click extends from the caret"
    );

    let start = at(&view, vcx, 0, 0.25);
    let to = at(&view, vcx, 9, 0.75);
    vcx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    assert_eq!(
        selection(&view, vcx),
        0..10,
        "a drag selects what it passes"
    );
}

#[gpui::test]
fn double_click_selects_a_word_triple_click_all(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 200., TEXT);
    let p = at(&view, vcx, 7, 0.5);
    press(vcx, p, MouseButton::Left, Modifiers::none(), 1);
    press(vcx, p, MouseButton::Left, Modifiers::none(), 2);
    assert_eq!(selection(&view, vcx), 6..10);
    press(vcx, p, MouseButton::Left, Modifiers::none(), 3);
    assert_eq!(selection(&view, vcx), 0..16);
}

#[gpui::test]
fn the_right_click_menu_skips_paste_with_an_empty_clipboard(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 200., TEXT);
    vcx.write_to_clipboard(ClipboardItem::new_string(String::new()));
    // Select "alpha", then right-click inside the selection: it stays.
    let a = at(&view, vcx, 0, 0.25);
    let h = at(&view, vcx, 4, 0.75);
    vcx.simulate_mouse_down(a, MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_move(h, MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_up(h, MouseButton::Left, Modifiers::none());
    assert_eq!(selection(&view, vcx), 0..5);
    let inside = at(&view, vcx, 2, 0.5);
    press(vcx, inside, MouseButton::Right, Modifiers::none(), 1);
    vcx.run_until_parked();
    assert_eq!(
        selection(&view, vcx),
        0..5,
        "a right-click in the selection keeps it"
    );

    // Cut, Copy, Paste (disabled: nothing to paste), Select All: the third Down lands on Select All.
    vcx.simulate_keystrokes("down down down enter");
    assert_eq!(selection(&view, vcx), 0..16, "Select All ran from the menu");
}

/// A rename commits on Blur, so the menu's own focus must not read as one.
#[gpui::test]
fn the_menu_is_not_a_blur_and_pastes_into_the_field(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 200., TEXT);
    vcx.write_to_clipboard(ClipboardItem::new_string("X".into()));
    let inside = at(&view, vcx, 6, 0.25);
    press(vcx, inside, MouseButton::Right, Modifiers::none(), 1);
    vcx.run_until_parked();
    // Nothing is selected, so Cut and Copy are disabled and the first row is Paste.
    vcx.simulate_keystrokes("down enter");
    vcx.run_until_parked();
    assert_eq!(value(&view, vcx), "alpha Xbeta gamma");
    assert_eq!(events(&view, vcx), [TextInputEvent::Change]);
    vcx.simulate_input("Y");
    assert_eq!(value(&view, vcx), "alpha XYbeta gamma", "the focus is back");
}

#[gpui::test]
fn leaving_the_menu_for_another_control_is_a_blur(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 200., TEXT);
    let inside = at(&view, vcx, 6, 0.25);
    press(vcx, inside, MouseButton::Right, Modifiers::none(), 1);
    vcx.run_until_parked();
    assert!(events(&view, vcx).is_empty());
    let other = view.read_with(vcx, |v, _| v.other.clone());
    vcx.update(|window, cx| window.focus(&other, cx));
    vcx.run_until_parked();
    let outside = point(gpui::px(400.), gpui::px(300.));
    press(vcx, outside, MouseButton::Left, Modifiers::none(), 1);
    vcx.run_until_parked();
    assert_eq!(events(&view, vcx), [TextInputEvent::Blur]);
}

#[gpui::test]
fn a_drag_after_a_double_click_takes_whole_words(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 200., TEXT);
    let beta = at(&view, vcx, 7, 0.5);
    press(vcx, beta, MouseButton::Left, Modifiers::none(), 1);
    vcx.simulate_event(MouseDownEvent {
        button: MouseButton::Left,
        position: beta,
        modifiers: Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    let gamma = at(&view, vcx, 12, 0.5);
    vcx.simulate_mouse_move(gamma, MouseButton::Left, Modifiers::none());
    assert_eq!(selection(&view, vcx), 6..16);
    let alpha = at(&view, vcx, 2, 0.5);
    vcx.simulate_mouse_move(alpha, MouseButton::Left, Modifiers::none());
    assert_eq!(selection(&view, vcx), 0..10);
    // Outside the field, the drag goes on.
    vcx.simulate_mouse_move(
        point(gpui::px(500.), gpui::px(300.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    assert_eq!(selection(&view, vcx), 6..16);
}
