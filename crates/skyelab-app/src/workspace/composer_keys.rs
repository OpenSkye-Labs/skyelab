//! The composer's key bindings: the `@` `/` `$` menu's keys, steer, interrupt, stop, stash and the docked question.

use super::composer_state::{MenuAccept, MenuClose, MenuDown, MenuUp, SteerPrompt};
use gpui::{App, KeyBinding};

const INTERRUPT_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-."
} else {
    "ctrl-."
};
const STOP_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-alt-."
} else {
    "ctrl-alt-."
};

/// Set on the composer while a menu is open, so its keys reach the menu before the text field.
pub(in crate::workspace) const MENU_CONTEXT: &str = "ComposerMenu";
const STEER_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-shift-enter"
} else {
    "ctrl-shift-enter"
};

/// Binds the composer's keys. The menu's override the text field's own up/down/enter/tab/escape: same depth, bound later.
pub(in crate::workspace) fn bind_keys(cx: &mut App) {
    let menu = Some("ComposerMenu > Input");
    cx.bind_keys([
        KeyBinding::new("up", MenuUp, menu),
        KeyBinding::new("down", MenuDown, menu),
        KeyBinding::new("enter", MenuAccept, menu),
        KeyBinding::new("tab", MenuAccept, menu),
        KeyBinding::new("escape", MenuClose, menu),
        KeyBinding::new(STEER_KEY, SteerPrompt, Some(super::thread::CONTEXT)),
        // Stop has no button (T3 has none either): the palette and this key.
        KeyBinding::new(
            INTERRUPT_KEY,
            super::thread::Interrupt,
            Some(super::thread::CONTEXT),
        ),
        KeyBinding::new(STOP_KEY, super::thread::Stop, Some(super::thread::CONTEXT)),
        KeyBinding::new(
            crate::workspace::ask_dock::FOCUS_QUESTION_KEY,
            crate::workspace::ask_dock::FocusQuestion,
            Some(super::thread::CONTEXT),
        ),
        KeyBinding::new(
            super::composer_stash::STASH_KEY,
            super::composer_stash::StashPrompt,
            Some(super::thread::CONTEXT),
        ),
    ]);
    super::composer_draft::bind_keys(cx);
    super::composer_voice::bind_keys(cx);
    super::composer_picker_keys::bind_keys(cx);
}
