//! Own UI kit, stage 1 (.agent/plans/2026-11-27-own-ui-kit.md), criterion 3: while a dialog is open, Tab and Shift-Tab
//! never leave it, though the element behind it is a tab stop Tab reaches before the dialog opens.

use super::dialog_test::{behind_focused, open, window};
use gpui::TestAppContext;

#[gpui::test]
fn tab_and_shift_tab_stay_inside_an_open_dialog(cx: &mut TestAppContext) {
    let (behind, vcx) = window(cx);
    // Without a dialog, Tab reaches the element behind: a trap that lets it through would show here.
    vcx.simulate_keystrokes("tab");
    assert!(
        behind_focused(&behind, vcx),
        "Tab reaches the background before any dialog"
    );

    open(vcx, "Rename thread");
    for key in ["tab", "shift-tab"] {
        for press in 1..=6 {
            vcx.simulate_keystrokes(key);
            assert!(
                !behind_focused(&behind, vcx),
                "{key} press {press} moved focus behind the dialog"
            );
            assert!(
                vcx.update(|window, cx| window.focused(cx).is_some()),
                "{key} press {press} left nothing focused"
            );
        }
    }
}
