//! Own UI kit, stage 6a (.agent/plans/2026-11-29-own-ui-kit-text-editing-6a.md), criterion 3: Enter emits PressEnter
//! and still reaches an ancestor's binding (a dialog's confirm); Escape is the ancestor's, after an IME composition.

use super::text_input_harness::{ParentEnter, ParentEscape, cmd, events, focused, input, value};
use crate::ui::TextInputEvent;
use gpui::{EntityInputHandler as _, KeyBinding, TestAppContext};

#[gpui::test]
fn enter_emits_and_propagates_escape_is_the_parents(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 300., "abc");
    vcx.update(|_, cx| {
        cx.bind_keys([
            KeyBinding::new("enter", ParentEnter, Some("FieldParent")),
            KeyBinding::new("escape", ParentEscape, Some("FieldParent")),
        ])
    });
    let (enters, escapes) = view.read_with(vcx, |v, _| (v.enters.clone(), v.escapes.clone()));

    vcx.simulate_keystrokes("enter");
    assert_eq!(
        events(&view, vcx),
        [TextInputEvent::PressEnter { secondary: false, shift: false }]
    );
    assert_eq!(enters.get(), 1, "Enter reaches the parent's binding too");
    assert_eq!(value(&view, vcx), "abc", "a single-line Enter inserts nothing");

    vcx.simulate_keystrokes(&cmd("enter"));
    assert_eq!(
        events(&view, vcx),
        [TextInputEvent::PressEnter { secondary: true, shift: false }]
    );
    vcx.simulate_keystrokes("shift-enter");
    assert_eq!(
        events(&view, vcx),
        [TextInputEvent::PressEnter { secondary: false, shift: true }]
    );

    vcx.simulate_keystrokes("escape");
    assert_eq!(escapes.get(), 1, "the field doesn't take Escape");
    assert_eq!(value(&view, vcx), "abc", "Escape clears nothing");

    // Mid composition, Escape ends the composition and keeps its text.
    let field = input(&view, vcx);
    field.update_in(vcx, |i, window, cx| {
        i.replace_and_mark_text_in_range(None, "ka", None, window, cx)
    });
    vcx.simulate_keystrokes("escape");
    let marked = field.update_in(vcx, |i, window, cx| i.marked_text_range(window, cx));
    assert_eq!(marked, None, "the mark is gone");
    assert_eq!(value(&view, vcx), "abcka", "the composed text stays");
}
