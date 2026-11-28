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
pub(crate) const REOPEN_CLOSED_KEY: &str = if MAC { "cmd-shift-t" } else { "ctrl-shift-t" };
pub(crate) const SEARCH_FILES_KEY: &str = if MAC { "cmd-shift-f" } else { "ctrl-shift-f" };

#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = skyelab, no_json)]
pub struct OpenSettingsSection {
    pub section: crate::settings::Section,
}

#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = skyelab, no_json)]
pub struct LogInAccount {
    pub id: String,
}

#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = skyelab, no_json)]
pub struct SelectProject {
    pub id: i64,
}

#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = skyelab, no_json)]
pub struct OpenThread {
    pub id: i64,
}

/// Binds the workspace keys. Call once at startup.
pub fn init(cx: &mut App) {
    #[cfg(test)]
    crate::test_wait::shut_down_at_test_end(cx);
    cx.set_global(super::device_agent::DeviceClaims::default());
    cx.on_action(|_: &CloseWindow, cx| click::close_active_window(cx));
    cx.bind_keys([
        KeyBinding::new(TOGGLE_SIDEBAR_KEY, ToggleSidebar, Some(CONTEXT)),
        KeyBinding::new(TOGGLE_BOTTOM_KEY, ToggleBottomPanel, Some(CONTEXT)),
        KeyBinding::new(NEW_TERMINAL_KEY, NewTerminal, Some(CONTEXT)),
        KeyBinding::new(OPEN_SETTINGS_KEY, OpenSettings, Some(CONTEXT)),
        KeyBinding::new(windows::NEW_WINDOW_KEY, NewWindow, Some(CONTEXT)),
        KeyBinding::new(OPEN_BROWSER_KEY, OpenBrowser, Some(CONTEXT)),
        KeyBinding::new(OPEN_DEVICES_KEY, OpenDevices, Some(CONTEXT)),
        KeyBinding::new(GO_BACK_KEY, GoBack, Some(CONTEXT)),
        KeyBinding::new(GO_FORWARD_KEY, GoForward, Some(CONTEXT)),
        KeyBinding::new(FILE_PICKER_KEY, files::OpenFilePicker, Some(CONTEXT)),
        KeyBinding::new(SEARCH_FILES_KEY, files::SearchFiles, Some(CONTEXT)),
        KeyBinding::new(REOPEN_CLOSED_KEY, reopen::ReopenClosedView, Some(CONTEXT)),
        KeyBinding::new(
            layout::TOGGLE_DETAILS_KEY,
            layout::ToggleThreadDetails,
            Some(CONTEXT),
        ),
        KeyBinding::new("f6", focus_areas::FocusNextArea, Some(CONTEXT)),
        KeyBinding::new("shift-f6", focus_areas::FocusPreviousArea, Some(CONTEXT)),
        KeyBinding::new("escape", focus_areas::FocusComposer, Some(CONTEXT)),
        // A shell (vim, less, readline) needs its Esc.
        KeyBinding::new("escape", NoAction, Some("Terminal")),
    ]);
    // Bindings run before a view sees the key, so off macOS ctrl-b/ctrl-j would never reach the shell.
    if !MAC {
        cx.bind_keys([
            KeyBinding::new(TOGGLE_SIDEBAR_KEY, NoAction, Some("Terminal")),
            KeyBinding::new(TOGGLE_BOTTOM_KEY, NoAction, Some("Terminal")),
            // ctrl-p is the shell's previous command.
            KeyBinding::new(FILE_PICKER_KEY, NoAction, Some("Terminal")),
            // ctrl-shift-t opens a tab in many terminal programs; the shell may want it.
            KeyBinding::new(REOPEN_CLOSED_KEY, NoAction, Some("Terminal")),
            // alt-left/right move by word in a shell.
            KeyBinding::new(GO_BACK_KEY, NoAction, Some("Terminal")),
            KeyBinding::new(GO_FORWARD_KEY, NoAction, Some("Terminal")),
        ]);
    }
    cx.bind_keys(field_bindings(MAC));
    crate::ui::bind_keys(cx);
    layout::bind_keys(cx);
    onboarding::bind_keys(cx);
    surfaces::bind_keys(cx);
    files::bind_keys(cx);
    thread_find::bind_keys(cx);
    skyelab_browser::bind_keys(cx);
    changes::bind_keys(cx);
    terminal_panel::bind_keys(cx);
    terminal_tab::bind_keys(cx);
    devices::bind_keys(cx);
    attention::init(cx);
    crate::palette::init(cx);
}

/// The workspace's keys inside a text field, on macOS (`mac`) or elsewhere: `Input` is gpui-component's text areas.
pub(crate) fn field_bindings(mac: bool) -> Vec<KeyBinding> {
    let fields = ["Input", crate::ui::text_input_keys::KEY_CONTEXT].map(Some);
    fields
        .into_iter()
        .flat_map(|field| {
            if mac {
                // A text area binds cmd-[ to Outdent and cmd-shift-f to Replace; the prompt is focused on most pages
                // and has nothing to replace, so history and the file search win.
                vec![
                    KeyBinding::new("cmd-[", GoBack, field),
                    KeyBinding::new("cmd-]", GoForward, field),
                    KeyBinding::new("cmd-shift-f", files::SearchFiles, field),
                ]
            } else {
                // In a text field, leaving the page on alt-left/right would surprise more than help.
                vec![
                    KeyBinding::new("alt-left", NoAction, field),
                    KeyBinding::new("alt-right", NoAction, field),
                ]
            }
        })
        .collect()
}
