//! Clicked buttons' actions and the menu's Close Window: both act on this window's Workspace wherever focus is.

use super::windows::Windows;
use gpui::{App, Window};

/// Runs a clicked button's action. `Window::dispatch_action` starts from the focused element, or from the window's
/// root (gpui-component's `Root`, above the Workspace) when that element isn't drawn any more (a menu that just
/// closed), so the Workspace's handlers never saw it. This starts from the focused element only while it sits under
/// this window's Workspace, else from the Workspace itself (docs/gpui/keyboard.md).
pub(crate) fn dispatch(action: Box<dyn gpui::Action>, window: &mut Window, cx: &mut App) {
    let handle = window.window_handle();
    // Deferred, like `Window::dispatch_action`: a click handler may run inside the Workspace's own update.
    window.defer(cx, move |window, cx| {
        let workspace = cx.try_global::<Windows>().and_then(|reg| {
            let r = reg.workspaces.iter().find(|r| r.window == handle)?;
            r.workspace.upgrade()
        });
        let Some(workspace) = workspace else {
            return window.dispatch_action(action, cx); // not a workspace window
        };
        let ws_focus = workspace.read(cx).focus.clone();
        let from = window
            .focused(cx)
            .filter(|_| ws_focus.contains_focused(window, cx))
            .unwrap_or(ws_focus);
        from.dispatch_action(action.as_ref(), window, cx);
    });
}

/// The menu's Close Window (T3's: no shortcut, since ⌘W closes a tab). App-wide, so it works wherever focus is; it
/// asks first as the window's close button does.
pub(super) fn close_active_window(cx: &mut App) {
    if let Some(w) = cx.active_window() {
        let _ = w.update(cx, |_, window, cx| {
            if super::quit_guard::may_close(window, cx) {
                window.remove_window();
            }
        });
    }
}
