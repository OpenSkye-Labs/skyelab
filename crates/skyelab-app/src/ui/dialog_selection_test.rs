//! Own UI kit, stage 1 (.agent/plans/2026-11-27-own-ui-kit.md), criterion 9: opening a dialog clears a text selection
//! behind it, so cmd-c in the dialog can't copy what the user selected on the page.

use super::dialog_test::{open, window};
use gpui::{Modifiers, MouseButton, TestAppContext, point, px};

#[gpui::test]
fn opening_a_dialog_clears_the_selection_behind_it(cx: &mut TestAppContext) {
    let (_, vcx) = window(cx);
    let text = vcx
        .debug_bounds("behind-text")
        .expect("the text behind is drawn");
    let (from, to) = (
        point(text.left() + px(2.), text.center().y),
        point(text.right() - px(2.), text.center().y),
    );
    vcx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    vcx.run_until_parked();
    assert!(
        vcx.update(gpui_base::TextSelection::has_selection),
        "the drag selected the text behind"
    );

    open(vcx, "Rename thread");
    assert!(
        !vcx.update(gpui_base::TextSelection::has_selection),
        "the dialog cleared the selection behind it"
    );
    vcx.simulate_keystrokes("cmd-c");
    let copied = vcx.update(|_, cx| cx.read_from_clipboard().and_then(|c| c.text()));
    assert!(
        copied.as_ref().is_none_or(|t| !t.contains("Text behind")),
        "cmd-c copied text from behind the dialog: {copied:?}"
    );
}
