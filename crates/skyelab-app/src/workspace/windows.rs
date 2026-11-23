//! Several workspace windows over one store (T3's multiple windows). Each window has its own page, sidebar scope and
//! docks; thread views are shared, so a thread runs one agent session app-wide however many windows show it.
//! The registry here also owns the views, so an agent keeps running when its window closes (macOS keeps the app
//! running without windows), and it hands the app-wide call queues (sharing, orchestration) to one window at a time.

use super::orchestrate::OrchCall;
use super::thread::ThreadView;
use super::{SharedStore, Workspace};
use crate::remote::RemoteCall;
use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, Focusable as _, Global, Pixels,
    WeakEntity, Window, WindowOptions, actions, point, px,
};
use std::rc::Rc;
use std::sync::Arc;

actions!(skyelab, [NewWindow]);

pub(crate) const NEW_WINDOW_KEY: &str = if super::MAC {
    "cmd-shift-n"
} else {
    "ctrl-shift-n"
};

type Setup = Rc<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

/// How the app opens and fills a workspace window; main sets it once. Tests leave it unset (plain options, no hooks).
#[derive(Clone)]
pub struct WindowKit {
    pub options: Rc<dyn Fn(Bounds<Pixels>) -> WindowOptions>,
    /// Hooks a new workspace to the app's sharing and environments.
    pub setup: Setup,
}

impl Global for WindowKit {}

struct Registered {
    window: AnyWindowHandle,
    workspace: WeakEntity<Workspace>,
    store: SharedStore,
}

/// Every workspace window and every thread view; the call queues and which window consumes them.
#[derive(Default)]
pub(crate) struct Windows {
    workspaces: Vec<Registered>,
    threads: Vec<Entity<ThreadView>>,
    remote_calls: Option<flume::Receiver<RemoteCall>>,
    orchestrate: Option<flume::Receiver<OrchCall>>,
    hub: Option<AnyWindowHandle>,
    /// The window-closed hook is installed (once).
    watching: bool,
}

impl Global for Windows {}

/// Main hands over the app-wide call queues; the first window (and later the next one) consumes them.
pub fn set_hub(
    remote_calls: Option<flume::Receiver<RemoteCall>>,
    orchestrate: Option<flume::Receiver<OrchCall>>,
    cx: &mut App,
) {
    let w = cx.default_global::<Windows>();
    w.remote_calls = remote_calls;
    w.orchestrate = orchestrate;
}

/// The workspace windows, in the order they opened.
pub fn window_handles(cx: &App) -> Vec<AnyWindowHandle> {
    cx.try_global::<Windows>()
        .map(|w| w.workspaces.iter().map(|r| r.window).collect())
        .unwrap_or_default()
}

#[cfg(test)]
pub(super) fn workspaces(cx: &App) -> Vec<Entity<Workspace>> {
    cx.global::<Windows>()
        .workspaces
        .iter()
        .filter_map(|r| r.workspace.upgrade())
        .collect()
}

/// Opens a workspace window at `bounds`: the kit's options and setup, then the call queues if no window has them.
pub fn open(
    bounds: Bounds<Pixels>,
    make: impl FnOnce(&mut Window, &mut Context<Workspace>) -> Workspace + 'static,
    cx: &mut App,
) -> anyhow::Result<Entity<Workspace>> {
    let kit = cx.try_global::<WindowKit>().cloned();
    let options = match &kit {
        Some(kit) => (kit.options)(bounds),
        None => WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
    };
    let mut made = None;
    cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| {
            let mut ws = make(window, cx);
            if let Some(kit) = &kit {
                (kit.setup)(&mut ws, window, cx);
            }
            ws.claim_hub(window, cx);
            ws
        });
        let focus = view.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        made = Some(view.clone());
        cx.new(|cx| gpui_component::Root::new(view, window, cx))
    })?;
    made.ok_or_else(|| anyhow::anyhow!("the window opened without its workspace"))
}

/// A window closed: it leaves the registry, and the next window on its store owns what it owned and serves the call
/// queues if it did.
fn window_closed(id: gpui::WindowId, cx: &mut App) {
    let reg = cx.default_global::<Windows>();
    let Some(at) = reg
        .workspaces
        .iter()
        .position(|r| r.window.window_id() == id)
    else {
        return;
    };
    let gone = reg.workspaces.remove(at);
    let held = reg.hub == Some(gone.window);
    if held {
        reg.hub = None;
    }
    let next = reg
        .workspaces
        .iter()
        .find(|r| Arc::ptr_eq(&r.store, &gone.store))
        .map(|r| (r.window, r.workspace.clone()));
    let all = reg.threads.clone();
    let orphans: Vec<_> = all
        .into_iter()
        .filter(|v| v.read(cx).window == gone.window)
        .collect();
    let Some((window, workspace)) = next else {
        return;
    };
    let _ = window.update(cx, |_, window, cx| {
        for view in orphans {
            view.update(cx, |v, cx| v.attach(window, cx));
        }
        if held {
            let _ = workspace.update(cx, |ws, cx| ws.claim_hub(window, cx));
        }
    });
}

impl Workspace {
    /// Joins the registry: adopts every thread view already open on this store, and leaves it when the window closes.
    pub(super) fn register_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let me = Registered {
            window: window.window_handle(),
            workspace: cx.weak_entity(),
            store: self.backend.store.clone(),
        };
        let reg = cx.default_global::<Windows>();
        reg.workspaces.push(me);
        let all = reg.threads.clone();
        let shared: Vec<_> = all
            .into_iter()
            .filter(|v| Arc::ptr_eq(&v.read(cx).backend.store, &self.backend.store))
            .collect();
        for view in shared {
            self.track_thread(&view, window, cx);
        }
        if !std::mem::replace(&mut cx.default_global::<Windows>().watching, true) {
            cx.on_window_closed(|cx, id| window_closed(id, cx)).detach();
        }
    }

    /// Serves the app-wide call queues when no window does yet.
    pub(super) fn claim_hub(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let reg = cx.default_global::<Windows>();
        if reg.hub.is_some_and(|h| h != self.window_handle) {
            return;
        }
        reg.hub = Some(self.window_handle);
        let (remote, orchestrate) = (reg.remote_calls.clone(), reg.orchestrate.clone());
        if let Some(calls) = remote {
            self.serve_remote_calls(calls, window, cx);
        }
        if let Some(calls) = orchestrate {
            self.set_orchestrator(calls, window, cx);
        }
    }

    /// Whether this window serves the call queues (and runs the background polls one window is enough for).
    pub(super) fn is_hub(&self, cx: &App) -> bool {
        cx.try_global::<Windows>()
            .is_none_or(|w| w.hub.is_none_or(|h| h == self.window_handle))
    }

    /// A view this window just opened: every other window on the store lists and follows it too.
    pub(super) fn share_thread(&self, view: &Entity<ThreadView>, cx: &mut Context<Self>) {
        let reg = cx.default_global::<Windows>();
        if reg.threads.contains(view) {
            return;
        }
        reg.threads.push(view.clone());
        let others = self.other_windows(cx);
        let view = view.clone();
        cx.defer(move |cx| {
            for (window, ws) in others {
                let view = view.clone();
                let _ = window.update(cx, |_, window, cx| {
                    ws.update(cx, |ws, cx| {
                        if !ws.open_threads.contains(&view) {
                            ws.track_thread(&view, window, cx);
                            ws.refresh_sidebar(cx);
                        }
                    })
                });
            }
        });
    }

    /// A thread closed here (archived, deleted): it leaves the registry and every other window.
    pub(super) fn unshare_thread(&self, view: &Entity<ThreadView>, cx: &mut Context<Self>) {
        cx.default_global::<Windows>().threads.retain(|v| v != view);
        let others = self.other_windows(cx);
        let view = view.clone();
        cx.defer(move |cx| {
            for (window, ws) in others {
                let view = view.clone();
                let _ = window.update(cx, |_, window, cx| {
                    ws.update(cx, |ws, cx| ws.forget_thread(&view, window, cx))
                });
            }
        });
    }

    /// Runs `f` on every other window's workspace over the same store (settings saved, accounts reloaded).
    pub(super) fn each_other_window(
        &self,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
    ) {
        let others = self.other_windows(cx);
        cx.defer(move |cx| {
            for (_, ws) in others {
                let _ = ws.update(cx, |ws, cx| f(ws, cx));
            }
        });
    }

    pub(super) fn has_other_windows(&self, cx: &App) -> bool {
        !self.other_windows(cx).is_empty()
    }

    fn other_windows(&self, cx: &App) -> Vec<(AnyWindowHandle, WeakEntity<Workspace>)> {
        cx.try_global::<Windows>()
            .map(|reg| {
                reg.workspaces
                    .iter()
                    .filter(|r| {
                        r.window != self.window_handle && Arc::ptr_eq(&r.store, &self.backend.store)
                    })
                    .map(|r| (r.window, r.workspace.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether another window shows thread `id` while it is the active window: the user is looking at it.
    pub(super) fn seen_elsewhere(&self, id: i64, cx: &App) -> bool {
        let active = cx.active_window();
        self.other_windows(cx).into_iter().any(|(window, ws)| {
            Some(window) == active
                && ws
                    .upgrade()
                    .is_some_and(|ws| ws.read(cx).shown_thread(cx) == Some(id))
        })
    }

    /// New Window: another workspace on the same store and accounts, cascaded from this one.
    pub(super) fn new_window(
        &mut self,
        _: &NewWindow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = window.bounds();
        let bounds = Bounds::new(at.origin + point(px(28.), px(28.)), at.size);
        let (folder, path, config, error, backend) = (
            self.folder.clone(),
            self.accounts_path.clone(),
            self.config.clone(),
            self.load_error.clone(),
            self.backend.clone(),
        );
        cx.defer(move |cx| {
            let opened = open(
                bounds,
                move |window, cx| Workspace::new(folder, path, config, error, backend, window, cx),
                cx,
            );
            if let Err(e) = opened {
                eprintln!("couldn't open a new window: {e:#}");
            }
        });
    }
}
