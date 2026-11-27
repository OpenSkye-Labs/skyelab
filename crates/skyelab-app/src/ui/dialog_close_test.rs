//! Own UI kit, stage 1 (.agent/plans/2026-11-27-own-ui-kit.md), criterion 4: Escape closes the dialog, and focus goes
//! back to what had it before the dialog opened.

use super::dialog_test::{behind_focused, dialogs, open, settle, window};
use crate::ui;
use gpui::TestAppContext;

#[gpui::test]
fn escape_closes_the_dialog_and_gives_focus_back(cx: &mut TestAppContext) {
    let (behind, vcx) = window(cx);
    vcx.update(|window, cx| {
        let focus = behind.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    open(vcx, "Rename thread");
    assert!(vcx.update(ui::has_active_dialog), "open");
    assert!(
        !behind_focused(&behind, vcx),
        "the dialog takes focus when it opens"
    );

    vcx.simulate_keystrokes("escape");
    settle(vcx);

    assert!(
        dialogs(vcx).is_empty(),
        "gone from the tree: {:?}",
        dialogs(vcx)
    );
    assert!(!vcx.update(ui::has_active_dialog), "no active dialog");
    assert!(behind_focused(&behind, vcx), "focus is back where it was");
}
