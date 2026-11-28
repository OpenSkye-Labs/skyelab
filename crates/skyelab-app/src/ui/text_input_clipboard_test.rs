//! Own UI kit, stage 6a (.agent/plans/2026-11-29-own-ui-kit-text-editing-6a.md), criterion 6: copy, cut and paste by
//! keystroke; a single-line paste drops line breaks and tabs, and is capped at 64 KiB.

use super::text_input_harness::{cmd, focused, key, selection, value};
use gpui::{ClipboardItem, TestAppContext};

fn clip(vcx: &mut gpui::VisualTestContext) -> Option<String> {
    vcx.read_from_clipboard().and_then(|c| c.text())
}

#[gpui::test]
fn copy_cut_and_paste(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 300., "alpha beta");
    vcx.simulate_keystrokes(key("alt-shift-left", "ctrl-shift-left"));
    assert_eq!(selection(&view, vcx), 6..10);
    vcx.simulate_keystrokes(&cmd("c"));
    assert_eq!(clip(vcx).as_deref(), Some("beta"));
    vcx.simulate_keystrokes(&cmd("x"));
    assert_eq!(value(&view, vcx), "alpha ");
    assert_eq!(clip(vcx).as_deref(), Some("beta"));

    vcx.write_to_clipboard(ClipboardItem::new_string("a\nb\tc\r".into()));
    vcx.simulate_keystrokes(&cmd("v"));
    assert_eq!(value(&view, vcx), "alpha abc", "line breaks and tabs dropped");
}

#[gpui::test]
fn copy_with_nothing_selected_leaves_the_clipboard(cx: &mut TestAppContext) {
    let (_, vcx) = focused(cx, 300., "alpha");
    vcx.write_to_clipboard(ClipboardItem::new_string("keep".into()));
    vcx.simulate_keystrokes(&cmd("c"));
    assert_eq!(clip(vcx).as_deref(), Some("keep"));
    vcx.simulate_keystrokes(&cmd("x"));
    assert_eq!(clip(vcx).as_deref(), Some("keep"));
}

#[gpui::test]
fn a_huge_paste_is_cut_at_64_kib_on_a_character_boundary(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 300., "");
    // € is three bytes: 64 KiB (65,536) is not a multiple of it, so the cut must step back to 65,535.
    vcx.write_to_clipboard(ClipboardItem::new_string("€".repeat(30_000)));
    vcx.simulate_keystrokes(&cmd("v"));
    let v = value(&view, vcx);
    assert_eq!(v.len(), 65_535);
    assert!(v.chars().all(|c| c == '€'));
}
