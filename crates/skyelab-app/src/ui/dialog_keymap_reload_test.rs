//! Review proof: Escape still closes a dialog after the keymap reloads.
use super::dialog_test::{dialogs, open, settle, window};
use gpui::TestAppContext;

#[gpui::test]
fn escape_still_closes_a_dialog_after_the_keymap_reloads(cx: &mut TestAppContext) {
    let (_, vcx) = window(cx);
    vcx.update(|_, cx| crate::keymap::init(None, cx));
    open(vcx, "First");
    vcx.simulate_keystrokes("escape");
    settle(vcx);
    assert!(dialogs(vcx).is_empty(), "Escape closes the first dialog");
    // The keybindings file changed: the watcher clears and re-binds.
    vcx.update(|_, cx| crate::keymap::load(Some(""), cx));
    open(vcx, "Second");
    vcx.simulate_keystrokes("escape");
    settle(vcx);
    assert_eq!(
        dialogs(vcx),
        Vec::<String>::new(),
        "Escape no longer closes a dialog after a keymap reload"
    );
}
