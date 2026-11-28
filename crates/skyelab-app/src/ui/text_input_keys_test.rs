//! Own UI kit, stage 6a (.agent/plans/2026-11-29-own-ui-kit-text-editing-6a.md), criterion 2: typing, caret moves,
//! deletes and Change events in our single-line input, by simulated keystrokes.

use super::text_input_harness::{
    cmd, cursor, events, field, focused, input, key, selection, value,
};
use crate::ui::TextInputEvent;
use gpui::{Focusable as _, TestAppContext};

#[gpui::test]
fn typing_moves_and_word_edits(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 300., "");
    vcx.simulate_input("hello world");
    assert_eq!(value(&view, vcx), "hello world");
    assert_eq!(cursor(&view, vcx), 11);
    let changes = events(&view, vcx)
        .into_iter()
        .filter(|e| *e == TextInputEvent::Change)
        .count();
    assert_eq!(changes, 11, "one Change per typed character");

    vcx.simulate_keystrokes(key("alt-left", "ctrl-left"));
    assert_eq!(cursor(&view, vcx), 6, "a word back");
    vcx.simulate_keystrokes(key("alt-shift-left", "ctrl-shift-left"));
    assert_eq!(selection(&view, vcx), 0..6, "select a word back");
    vcx.simulate_keystrokes("backspace");
    assert_eq!(value(&view, vcx), "world");
    assert_eq!(cursor(&view, vcx), 0);

    vcx.simulate_keystrokes(&cmd("a"));
    assert_eq!(selection(&view, vcx), 0..5, "select all");
    vcx.simulate_input("x");
    assert_eq!(value(&view, vcx), "x", "typing replaces the selection");
}

#[gpui::test]
fn line_moves_and_selections(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 300., "abc def");
    assert_eq!(cursor(&view, vcx), 7, "set_value leaves the caret at the end");
    vcx.simulate_keystrokes("left left");
    assert_eq!(cursor(&view, vcx), 5);
    vcx.simulate_keystrokes("right");
    assert_eq!(cursor(&view, vcx), 6);
    vcx.simulate_keystrokes("home");
    assert_eq!(cursor(&view, vcx), 0);
    vcx.simulate_keystrokes("shift-end");
    assert_eq!(selection(&view, vcx), 0..7);
    vcx.simulate_keystrokes("end");
    assert_eq!(selection(&view, vcx), 7..7);
    vcx.simulate_keystrokes("shift-home");
    assert_eq!(selection(&view, vcx), 0..7);
    vcx.simulate_keystrokes("left");
    assert_eq!(selection(&view, vcx), 0..0, "left collapses to the start");
}

#[gpui::test]
fn deletes_by_character_word_and_line(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 300., "foo bar baz");
    vcx.simulate_keystrokes(key("alt-backspace", "ctrl-backspace"));
    assert_eq!(value(&view, vcx), "foo bar ");
    vcx.simulate_keystrokes("home delete");
    assert_eq!(value(&view, vcx), "oo bar ");
    vcx.simulate_keystrokes(key("alt-delete", "ctrl-delete"));
    assert_eq!(value(&view, vcx), " bar ");
    if cfg!(target_os = "macos") {
        vcx.simulate_keystrokes("end cmd-backspace");
        assert_eq!(value(&view, vcx), "", "cmd-backspace deletes to the line start");
    }
}

#[gpui::test]
fn set_value_is_silent_and_not_undoable(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 300., "");
    vcx.simulate_input("typed");
    events(&view, vcx);
    let field = input(&view, vcx);
    vcx.update(|window, cx| field.update(cx, |i, cx| i.set_value("abc", window, cx)));
    vcx.run_until_parked();
    assert!(events(&view, vcx).is_empty(), "set_value emits no Change");
    assert_eq!(cursor(&view, vcx), 3);
    vcx.simulate_keystrokes(&cmd("z"));
    assert_eq!(value(&view, vcx), "abc", "set_value cleared the undo history");
}

#[gpui::test]
fn a_grapheme_cluster_is_one_step(cx: &mut TestAppContext) {
    let thumb = "👍🏽";
    let (view, vcx) = focused(cx, 300., &format!("a{thumb}b"));
    vcx.simulate_keystrokes("left left");
    assert_eq!(cursor(&view, vcx), 1, "one left over the emoji with its skin tone");
    vcx.simulate_keystrokes("right");
    assert_eq!(cursor(&view, vcx), 1 + thumb.len());

    let accented = "e\u{301}x";
    let field = input(&view, vcx);
    vcx.update(|window, cx| field.update(cx, |i, cx| i.set_value(accented, window, cx)));
    vcx.simulate_keystrokes("left left");
    assert_eq!(cursor(&view, vcx), 0, "e and its combining accent are one step");
    vcx.simulate_keystrokes("delete");
    assert_eq!(value(&view, vcx), "x", "delete takes the whole cluster");
}

#[gpui::test]
fn tab_reaches_an_enabled_field(cx: &mut TestAppContext) {
    let (view, vcx) = field(cx, 300., "");
    let field = input(&view, vcx);
    let other = view.read_with(vcx, |v, _| v.other.clone());
    vcx.update(|window, cx| window.focus(&other, cx));
    vcx.simulate_keystrokes("shift-tab");
    let focused = vcx.update(|window, cx| field.focus_handle(cx).is_focused(window));
    assert!(focused, "Tab stops at the field");
}

#[gpui::test]
fn a_disabled_field_takes_no_focus(cx: &mut TestAppContext) {
    let (view, vcx) = field(cx, 300., "x");
    let field = input(&view, vcx);
    vcx.update(|window, cx| field.update(cx, |i, cx| i.set_disabled(true, window, cx)));
    let other = view.read_with(vcx, |v, _| v.other.clone());
    vcx.update(|window, cx| window.focus(&other, cx));
    vcx.simulate_keystrokes("shift-tab");
    let focused = vcx.update(|window, cx| field.focus_handle(cx).is_focused(window));
    assert!(!focused, "Tab skips a disabled field");
}

/// Criterion 14's first half: one Focus, then one Blur; a click inside the focused field sends neither.
#[gpui::test]
fn focus_then_blur_each_emit_once(cx: &mut TestAppContext) {
    let (view, vcx) = field(cx, 300., "abc");
    let field = input(&view, vcx);
    vcx.update(|window, cx| {
        let focus = field.focus_handle(cx);
        window.focus(&focus, cx);
    });
    vcx.run_until_parked();
    assert_eq!(events(&view, vcx), [TextInputEvent::Focus]);

    let inside = super::text_input_harness::range_bounds(&view, vcx, 1..2).center();
    vcx.simulate_click(inside, gpui::Modifiers::none());
    assert!(events(&view, vcx).is_empty(), "a click inside keeps the focus");

    let other = view.read_with(vcx, |v, _| v.other.clone());
    vcx.update(|window, cx| window.focus(&other, cx));
    vcx.run_until_parked();
    assert_eq!(events(&view, vcx), [TextInputEvent::Blur]);
}
