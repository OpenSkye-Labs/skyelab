//! Own UI kit, stage 6a (.agent/plans/2026-11-29-own-ui-kit-text-editing-6a.md), criterion 5: undo and redo by
//! keystroke; a typing run is one step, a paste is its own, undo gives back the selection the edit replaced.

use super::text_input_harness::{cmd, focused, key, selection, value};
use gpui::{ClipboardItem, TestAppContext};

fn redo() -> String {
    key("cmd-shift-z", "ctrl-shift-z").to_string()
}

#[gpui::test]
fn a_typing_run_is_one_step(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 300., "");
    vcx.simulate_input("one two");
    vcx.simulate_keystrokes(&cmd("z"));
    assert_eq!(value(&view, vcx), "");
    vcx.simulate_keystrokes(&redo());
    assert_eq!(value(&view, vcx), "one two");
}

#[gpui::test]
fn a_paste_is_a_step_of_its_own(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 300., "");
    vcx.simulate_input("ab");
    vcx.write_to_clipboard(ClipboardItem::new_string("XY".into()));
    vcx.simulate_keystrokes(&cmd("v"));
    assert_eq!(value(&view, vcx), "abXY");
    vcx.simulate_keystrokes(&cmd("z"));
    assert_eq!(value(&view, vcx), "ab");
    vcx.simulate_keystrokes(&cmd("z"));
    assert_eq!(value(&view, vcx), "");
}

#[gpui::test]
fn undo_restores_the_replaced_selection(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 300., "hello");
    vcx.simulate_keystrokes(&cmd("a"));
    vcx.simulate_input("z");
    assert_eq!(value(&view, vcx), "z");
    vcx.simulate_keystrokes(&cmd("z"));
    assert_eq!(value(&view, vcx), "hello");
    assert_eq!(
        selection(&view, vcx),
        0..5,
        "the selection comes back with the text"
    );
}
