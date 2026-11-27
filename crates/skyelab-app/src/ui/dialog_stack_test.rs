//! Own UI kit, stage 1 (.agent/plans/2026-11-27-own-ui-kit.md), criterion 5: with a dialog over another, Escape closes
//! only the top one, and focus lands in the one below.

use super::dialog_test::{behind_focused, dialogs, open, settle, window};
use gpui::TestAppContext;

#[gpui::test]
fn escape_closes_only_the_top_dialog(cx: &mut TestAppContext) {
    let (behind, vcx) = window(cx);
    open(vcx, "First");
    open(vcx, "Second");
    assert_eq!(dialogs(vcx), ["First", "Second"]);

    vcx.simulate_keystrokes("escape");
    settle(vcx);

    assert_eq!(dialogs(vcx), ["First"], "only the top dialog closed");
    assert!(
        !behind_focused(&behind, vcx),
        "focus didn't fall behind both dialogs"
    );
    assert!(
        vcx.update(|window, cx| window.focused(cx).is_some()),
        "focus is in the remaining dialog"
    );
}
