//! The workspace's key bindings.

use super::*;
use gpui::{App, KeyBinding, NoAction};

// `KeyBinding::new` can't resolve `secondary-`, so pick per OS.
pub(super) const MAC: bool = cfg!(target_os = "macos");
pub(crate) const TOGGLE_SIDEBAR_KEY: &str = if MAC { "cmd-b" } else { "ctrl-b" };
pub(crate) const TOGGLE_BOTTOM_KEY: &str = if MAC { "cmd-j" } else { "ctrl-j" };
pub(crate) const NEW_TERMINAL_KEY: &str = if MAC { "cmd-alt-t" } else { "ctrl-alt-t" };
pub(crate) const OPEN_SETTINGS_KEY: &str = if MAC { "cmd-," } else { "ctrl-," };
pub(crate) const OPEN_BROWSER_KEY: &str = if MAC { "cmd-alt-b" } else { "ctrl-alt-b" };
pub(crate) const OPEN_DEVICES_KEY: &str = if MAC { "cmd-shift-d" } else { "ctrl-alt-d" };
pub(crate) const GO_BACK_KEY: &str = if MAC { "cmd-[" } else { "alt-left" };
pub(crate) const GO_FORWARD_KEY: &str = if MAC { "cmd-]" } else { "alt-right" };
pub(crate) const FILE_PICKER_KEY: &str = if MAC { "cmd-p" } else { "ctrl-p" };
pub(crate) const SEARCH_FILES_KEY: &str = if MAC { "cmd-shift-f" } else { "ctrl-shift-f" };

/// Binds the workspace keys. Call once at startup.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(TOGGLE_SIDEBAR_KEY, ToggleSidebar, Some(CONTEXT)),
        KeyBinding::new(TOGGLE_BOTTOM_KEY, ToggleBottomPanel, Some(CONTEXT)),
        KeyBinding::new(NEW_TERMINAL_KEY, NewTerminal, Some(CONTEXT)),
        KeyBinding::new(OPEN_SETTINGS_KEY, OpenSettings, Some(CONTEXT)),
        KeyBinding::new(OPEN_BROWSER_KEY, OpenBrowser, Some(CONTEXT)),
        KeyBinding::new(OPEN_DEVICES_KEY, OpenDevices, Some(CONTEXT)),
        KeyBinding::new(GO_BACK_KEY, GoBack, Some(CONTEXT)),
        KeyBinding::new(GO_FORWARD_KEY, GoForward, Some(CONTEXT)),
        KeyBinding::new(FILE_PICKER_KEY, files::OpenFilePicker, Some(CONTEXT)),
        KeyBinding::new(SEARCH_FILES_KEY, files::SearchFiles, Some(CONTEXT)),
    ]);
    // Bindings run before a view sees the key, so off macOS ctrl-b/ctrl-j would never reach the shell.
    if !MAC {
        cx.bind_keys([
            KeyBinding::new(TOGGLE_SIDEBAR_KEY, NoAction, Some("Terminal")),
            KeyBinding::new(TOGGLE_BOTTOM_KEY, NoAction, Some("Terminal")),
            // ctrl-p is the shell's previous command.
            KeyBinding::new(FILE_PICKER_KEY, NoAction, Some("Terminal")),
            // alt-left/right move by word in a shell; in a text field they would surprise more than help.
            KeyBinding::new(GO_BACK_KEY, NoAction, Some("Terminal")),
            KeyBinding::new(GO_FORWARD_KEY, NoAction, Some("Terminal")),
            KeyBinding::new(GO_BACK_KEY, NoAction, Some("Input")),
            KeyBinding::new(GO_FORWARD_KEY, NoAction, Some("Input")),
        ]);
    } else {
        // A text field binds cmd-[ to Outdent; the prompt is focused on most pages, so history wins there.
        cx.bind_keys([
            KeyBinding::new(GO_BACK_KEY, GoBack, Some("Input")),
            KeyBinding::new(GO_FORWARD_KEY, GoForward, Some("Input")),
            // A text field binds cmd-shift-f to Replace; the prompt and the read-only preview have nothing to replace.
            KeyBinding::new(SEARCH_FILES_KEY, files::SearchFiles, Some("Input")),
        ]);
    }
    layout::bind_keys(cx);
    files::bind_keys(cx);
    thread_find::bind_keys(cx);
    attention::init(cx);
    crate::palette::init(cx);
}
