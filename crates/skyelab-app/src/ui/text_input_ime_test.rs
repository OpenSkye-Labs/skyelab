//! Own UI kit, stage 6a (.agent/plans/2026-11-29-own-ui-kit-text-editing-6a.md), criterion 4: IME composition through
//! `EntityInputHandler`, the calls the platform makes (GPUI's test window has no input method), with UTF-16 ranges.

use super::text_input_harness::{cmd, focused, input, value};
use gpui::{EntityInputHandler as _, TestAppContext, UTF16Selection};

#[gpui::test]
fn pinyin_composition_marks_then_commits_as_one_undo(cx: &mut TestAppContext) {
    // 😀 is two UTF-16 units and four bytes: byte and UTF-16 offsets differ from here on.
    let (view, vcx) = focused(cx, 300., "😀a");
    let field = input(&view, vcx);
    let mark = |text: &'static str, vcx: &mut gpui::VisualTestContext| {
        field.update_in(vcx, |i, window, cx| {
            i.replace_and_mark_text_in_range(None, text, None, window, cx)
        });
        field.update_in(vcx, |i, window, cx| i.marked_text_range(window, cx))
    };
    assert_eq!(mark("n", vcx), Some(3..4));
    assert_eq!(value(&view, vcx), "😀an");
    assert_eq!(mark("ni", vcx), Some(3..5));
    assert_eq!(value(&view, vcx), "😀ani");
    assert_eq!(mark("你", vcx), Some(3..4));
    assert_eq!(value(&view, vcx), "😀a你");

    field.update_in(vcx, |i, window, cx| i.unmark_text(window, cx));
    let marked = field.update_in(vcx, |i, window, cx| i.marked_text_range(window, cx));
    assert_eq!(marked, None);
    assert_eq!(value(&view, vcx), "😀a你");
    let sel = field.update_in(vcx, |i, window, cx| i.selected_text_range(false, window, cx));
    assert_eq!(
        sel.map(|s: UTF16Selection| (s.range, s.reversed)),
        Some((4..4, false)),
        "the caret after 你, in UTF-16"
    );

    vcx.simulate_keystrokes(&cmd("z"));
    assert_eq!(value(&view, vcx), "😀a", "one undo takes the whole composition");
}

#[gpui::test]
fn utf16_ranges_convert_to_bytes(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 300., "😀a你b");
    let field = input(&view, vcx);
    let mut adjusted = None;
    let text = field.update_in(vcx, |i, window, cx| {
        i.text_for_range(0..2, &mut adjusted, window, cx)
    });
    assert_eq!(text.as_deref(), Some("😀"));
    assert_eq!(adjusted, Some(0..2));

    let text = field.update_in(vcx, |i, window, cx| {
        i.text_for_range(3..4, &mut adjusted, window, cx)
    });
    assert_eq!(text.as_deref(), Some("你"));

    // A range that splits the emoji's surrogate pair widens to the whole character.
    let text = field.update_in(vcx, |i, window, cx| {
        i.text_for_range(1..3, &mut adjusted, window, cx)
    });
    assert_eq!(text.as_deref(), Some("😀a"));
    assert_eq!(adjusted, Some(0..3));

    field.update_in(vcx, |i, window, cx| {
        i.replace_text_in_range(Some(0..2), "x", window, cx)
    });
    assert_eq!(value(&view, vcx), "xa你b", "a UTF-16 range replaces the emoji");
}

#[gpui::test]
fn bounds_for_range_is_the_caret(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 300., "abc");
    let field = input(&view, vcx);
    let (text, caret) = field.read_with(vcx, |i, _| (i.text_bounds(), i.caret_bounds()));
    let text = text.expect("drawn");
    let caret = caret.expect("focused, so a caret");
    let rect = field.update_in(vcx, |i, window, cx| i.bounds_for_range(3..3, text, window, cx));
    let rect = rect.expect("bounds for the caret position");
    assert_eq!(rect.origin.x, caret.origin.x, "the IME window goes at the caret");
    assert!(rect.size.height > gpui::px(0.));
}
