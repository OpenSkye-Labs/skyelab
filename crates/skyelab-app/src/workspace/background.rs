//! Keep running in background: every window closes but the process, its agents and the app-wide work stay. With no
//! visible window, a hidden hub window (`show: false`) serves orchestration calls, remote calls, the scheduler and
//! OS notifications (docs/gpui/windows.md). macOS opens it whenever the last window closes; elsewhere only in
//! background mode, since there closing the last window quits. In background mode the app quits by itself once
//! nothing has been working or waiting on the user for [`IDLE_GRACE`]. A visible window opening ends it.

use super::windows::{self, Windows, window_handles};
use super::{OpenThread, Workspace};
use gpui::{AnyWindowHandle, App, Bounds, Global, Task, WeakEntity, px, size};
use std::time::Duration;

/// How long nothing may work or wait before a background copy quits: a follow-up turn or a delegated child starts
/// well within it.
pub(crate) const IDLE_GRACE: Duration = Duration::from_secs(30);
pub(crate) const IDLE_POLL: Duration = Duration::from_secs(5);

/// Background mode: on from "Keep running in background" until a visible window opens.
#[derive(Default)]
struct Background {
    on: bool,
    _idle: Option<Task<()>>,
}

impl Global for Background {}

/// SkyeLab runs in background mode now.
pub(crate) fn is_on(cx: &App) -> bool {
    cx.try_global::<Background>().is_some_and(|b| b.on)
}

/// Whether a background copy must stay up: a thread or side chat works, starts or waits on the user, or a scheduled
/// run is starting or comes due by the next tick. The quit guard asks on the same work.
pub(crate) fn is_busy(cx: &App) -> bool {
    super::quit_guard::blockers(cx)
        .iter()
        .any(|(_, b)| b.is_work())
        || super::scheduled::due_soon(cx)
}

/// Turns background mode on (the windows close by themselves or through [`enter`]) and starts the idle quit.
pub(crate) fn turn_on(cx: &mut App) {
    if is_on(cx) {
        return;
    }
    let idle = cx.spawn(async move |cx| {
        let mut idle_since = None;
        loop {
            cx.background_executor().timer(IDLE_POLL).await;
            let quit = cx.update(|cx| {
                let now = cx.background_executor().now();
                if is_busy(cx) {
                    idle_since = None;
                    return false;
                }
                now - *idle_since.get_or_insert(now) >= IDLE_GRACE
            });
            if quit {
                cx.update(|cx| {
                    if !crate::updates::apply_if_deferred(cx) {
                        super::quit_guard::quit_now(cx);
                    }
                });
                return;
            }
        }
    });
    cx.set_global(Background {
        on: true,
        _idle: Some(idle),
    });
}

/// "Keep running in background": every visible window closes, the hidden hub takes over their work.
pub(crate) fn enter(cx: &mut App) {
    turn_on(cx);
    // Deferred: the dialog's click runs inside its window's update, where that window can't be removed.
    cx.defer(|cx| {
        for window in window_handles(cx) {
            let _ = window.update(cx, |_, window, _| window.remove_window());
        }
    });
}

/// A visible window opened: background mode ends, and the hidden hub closes and hands it the work.
pub(super) fn window_shown(cx: &mut App) {
    if is_on(cx) {
        cx.set_global(Background::default()); // drops the idle quit
    }
    let hidden: Vec<AnyWindowHandle> = cx
        .try_global::<Windows>()
        .map(|w| {
            let hubs = w.workspaces.iter().filter(|r| r.hidden);
            hubs.map(|r| r.window).collect()
        })
        .unwrap_or_default();
    if hidden.is_empty() {
        return;
    }
    cx.defer(move |cx| {
        for window in hidden {
            let _ = window.update(cx, |_, window, _| window.remove_window());
        }
    });
}

/// The last window on its store closed (`gone`, still alive during the close): on macOS or in background mode, a
/// hidden hub on the same store and accounts takes over. Off macOS outside background mode, nothing does, so GPUI
/// quits as before.
pub(super) fn keep_hub(
    gone: &WeakEntity<Workspace>,
    cx: &mut App,
) -> Option<(AnyWindowHandle, WeakEntity<Workspace>)> {
    keep_hub_on(super::keys::MAC, gone, cx)
}

pub(super) fn keep_hub_on(
    mac: bool,
    gone: &WeakEntity<Workspace>,
    cx: &mut App,
) -> Option<(AnyWindowHandle, WeakEntity<Workspace>)> {
    if !mac && !is_on(cx) {
        return None;
    }
    let ws = gone.upgrade()?;
    let ws = ws.read(cx);
    let (folder, path, config, error, backend) = (
        ws.folder.clone(),
        ws.accounts_path.clone(),
        ws.config.clone(),
        ws.load_error.clone(),
        ws.backend.clone(),
    );
    let bounds = Bounds::centered(None, size(px(800.), px(560.)), cx);
    let opened = windows::open_with(
        bounds,
        true,
        move |window, cx| Workspace::new(folder, path, config, error, backend, window, cx),
        cx,
    );
    match opened {
        Ok(hub) => Some((hub.read(cx).window_handle, hub.downgrade())),
        Err(e) => {
            eprintln!("skyelab: couldn't open the background hub: {e:#}");
            None
        }
    }
}

/// A notification from the hidden hub was clicked: a visible window opens (the saved ones, as the Dock does) and
/// shows thread `id`.
pub(super) fn show_thread(id: i64, cx: &mut App) {
    if window_handles(cx).is_empty()
        && let Some(open) = cx.try_global::<crate::FreshWindow>().map(|f| f.0.clone())
    {
        open(cx);
    }
    let Some(window) = window_handles(cx).first().copied() else {
        return;
    };
    let ws = cx.try_global::<Windows>().and_then(|w| {
        let r = w.workspaces.iter().find(|r| r.window == window)?;
        r.workspace.upgrade()
    });
    let Some(ws) = ws else {
        return;
    };
    // Deferred: the window that just opened may be mid-update.
    cx.defer(move |cx| {
        let _ = window.update(cx, |_, window, cx| {
            window.activate_window();
            ws.update(cx, |ws, cx| ws.open_thread(&OpenThread { id }, window, cx));
        });
    });
}
