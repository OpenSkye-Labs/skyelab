//! A composition sends one Change when it commits, never one per marked step, so the palette and search lists never
//! filter on half-typed Pinyin or Japanese (as gpui-component's field did).

use super::text_input_harness::{events, focused, input, value};
use crate::ui::TextInputEvent;
use gpui::{EntityInputHandler as _, TestAppContext, VisualTestContext};

fn mark(
    view: &gpui::Entity<super::text_input_harness::Field>,
    vcx: &mut VisualTestContext,
    text: &str,
) {
    input(view, vcx).update_in(vcx, |i, window, cx| {
        i.replace_and_mark_text_in_range(None, text, None, window, cx)
    });
}

fn changes(
    view: &gpui::Entity<super::text_input_harness::Field>,
    vcx: &mut VisualTestContext,
) -> usize {
    events(view, vcx)
        .into_iter()
        .filter(|e| *e == TextInputEvent::Change)
        .count()
}

#[gpui::test]
fn marked_steps_send_nothing_and_the_commit_sends_one_change(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 300., "a");
    for step in ["n", "ni", "nih"] {
        mark(&view, vcx, step);
    }
    assert_eq!(changes(&view, vcx), 0, "no Change while marked");
    input(&view, vcx).update_in(vcx, |i, window, cx| {
        i.replace_text_in_range(None, "你", window, cx)
    });
    assert_eq!(value(&view, vcx), "a你");
    assert_eq!(changes(&view, vcx), 1, "one Change on commit");
}

#[gpui::test]
fn unmarking_sends_one_change_and_a_cancelled_composition_none(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 300., "a");
    mark(&view, vcx, "ka");
    input(&view, vcx).update_in(vcx, |i, window, cx| i.unmark_text(window, cx));
    assert_eq!(changes(&view, vcx), 1, "the kept composition is a change");

    mark(&view, vcx, "s");
    mark(&view, vcx, "");
    input(&view, vcx).update_in(vcx, |i, window, cx| i.unmark_text(window, cx));
    assert_eq!(value(&view, vcx), "aka");
    assert_eq!(changes(&view, vcx), 0, "cancelled back to the old text");

    mark(&view, vcx, "x");
    vcx.simulate_keystrokes("escape");
    assert_eq!(changes(&view, vcx), 1, "Escape keeps the text, one Change");
}
