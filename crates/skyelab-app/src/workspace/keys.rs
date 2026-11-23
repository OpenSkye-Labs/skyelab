//! The workspace's key bindings: per-OS keys and `init`, which binds them once at startup.

use super::*;

// `KeyBinding::new` can't resolve `secondary-`, so pick per OS.
pub(super) const MAC: bool = cfg!(target_os = "macos");
pub(crate) const TOGGLE_SIDEBAR_KEY: &str = if MAC { "cmd-b" } else { "ctrl-b" };
pub(crate) const TOGGLE_BOTTOM_KEY: &str = if MAC { "cmd-j" } else { "ctrl-j" };
pub(crate) const NEW_TERMINAL_KEY: &str = if MAC { "cmd-alt-t" } else { "ctrl-alt-t" };
pub(crate) const OPEN_SETTINGS_KEY: &str = if MAC { "cmd-," } else { "ctrl-," };
pub(crate) const OPEN_BROWSER_KEY: &str = if MAC { "cmd-alt-b" } else { "ctrl-alt-b" };
pub(crate) const OPEN_DEVICES_KEY: &str = if MAC { "cmd-shift-d" } else { "ctrl-alt-d" };

/// Binds the workspace keys. Call once at startup.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(TOGGLE_SIDEBAR_KEY, ToggleSidebar, Some(CONTEXT)),
        KeyBinding::new(TOGGLE_BOTTOM_KEY, ToggleBottomPanel, Some(CONTEXT)),
        KeyBinding::new(NEW_TERMINAL_KEY, NewTerminal, Some(CONTEXT)),
        KeyBinding::new(OPEN_SETTINGS_KEY, OpenSettings, Some(CONTEXT)),
        KeyBinding::new(OPEN_BROWSER_KEY, OpenBrowser, Some(CONTEXT)),
        KeyBinding::new(OPEN_DEVICES_KEY, OpenDevices, Some(CONTEXT)),
    ]);
    // Bindings run before a view sees the key, so off macOS ctrl-b/ctrl-j would never reach the shell.
    if !MAC {
        cx.bind_keys([
            KeyBinding::new(TOGGLE_SIDEBAR_KEY, NoAction, Some("Terminal")),
            KeyBinding::new(TOGGLE_BOTTOM_KEY, NoAction, Some("Terminal")),
        ]);
    }
    layout::bind_keys(cx);
    attention::init(cx);
    crate::palette::init(cx);
}
