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
use skyelab_core::WindowLayout;
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

pub(super) struct Registered {
    window: AnyWindowHandle,
    pub(super) workspace: WeakEntity<Workspace>,
    store: SharedStore,
    /// Its frame, panel and page as last laid out: every layout save writes each open window's.
    layout: Option<WindowLayout>,
}

/// Every workspace window and every thread view; the call queues and which window consumes them.
#[derive(Default)]
pub(crate) struct Windows {
    pub(super) workspaces: Vec<Registered>,
    pub(super) threads: Vec<Entity<ThreadView>>,
    remote_calls: Option<flume::Receiver<RemoteCall>>,
    orchestrate: Option<flume::Receiver<OrchCall>>,
    hub: Option<AnyWindowHandle>,
    /// The window-closed hook is installed (once).
    watching: bool,
    /// The app is quitting: windows closing now stay in the saved list.
    quitting: bool,
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

/// The window that serves the app-wide work (the scheduler), else the first one open.
pub(super) fn hub_workspace(cx: &App) -> Option<(AnyWindowHandle, Entity<Workspace>)> {
    let reg = cx.try_global::<Windows>()?;
    let pick = |r: &&Registered| reg.hub.is_none_or(|h| h == r.window);
    let r = reg.workspaces.iter().find(pick)?;
    Some((r.window, r.workspace.upgrade()?))
}

/// The workspace windows, in the order they opened.
pub fn window_handles(cx: &App) -> Vec<AnyWindowHandle> {
    cx.try_global::<Windows>()
        .map(|w| w.workspaces.iter().map(|r| r.window).collect())
        .unwrap_or_default()
}

/// Every open window's layout, in the order the windows opened.
pub(super) fn open_layouts(cx: &App) -> Vec<WindowLayout> {
    cx.try_global::<Windows>()
        .map(|reg| {
            reg.workspaces
                .iter()
                .filter_map(|r| r.layout.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Keeps `window`'s current layout for the next save.
pub(super) fn set_layout(window: AnyWindowHandle, layout: WindowLayout, cx: &mut App) {
    let reg = cx.default_global::<Windows>();
    if let Some(r) = reg.workspaces.iter_mut().find(|r| r.window == window) {
        r.layout = Some(layout);
    }
}

/// The app is quitting (see `Windows::quitting`).
pub(super) fn quitting(cx: &mut App) {
    cx.default_global::<Windows>().quitting = true;
}

#[cfg(test)]
pub(super) fn workspaces(cx: &App) -> Vec<Entity<Workspace>> {
    let Some(reg) = cx.try_global::<Windows>() else {
        return Vec::new();
    };
    reg.workspaces
        .iter()
        .filter_map(|r| r.workspace.upgrade())
        .collect()
}

/// Opens every window in `saved` (frame and page) as the last session left them, or one window when there are none.
/// `make` builds window `n`'s workspace; each saves its layout to `settings` from then on.
pub fn restore_windows(
    saved: Vec<WindowLayout>,
    settings: Option<std::path::PathBuf>,
    displays: &[Bounds<Pixels>],
    visible: Bounds<Pixels>,
    make: impl Fn(usize, &mut Window, &mut Context<Workspace>) -> Workspace + 'static,
    cx: &mut App,
) -> anyhow::Result<Vec<Entity<Workspace>>> {
    let make = Rc::new(make);
    let saved: Vec<Option<WindowLayout>> = if saved.is_empty() {
        vec![None]
    } else {
        saved.into_iter().map(Some).collect()
    };
    let (mut opened, mut failed) = (Vec::new(), None);
    for (n, layout) in saved.into_iter().enumerate() {
        let bounds = crate::window_layout::initial_bounds(layout.as_ref(), displays, visible);
        let (make, settings) = (make.clone(), settings.clone());
        let made = open(
            bounds,
            move |window, cx| {
                let mut ws = make(n, window, cx);
                ws.restore_layout(settings, layout.as_ref(), window, cx);
                ws
            },
            cx,
        );
        // One window that can't open doesn't take the others down; only none opening is an error.
        match made {
            Ok(ws) => opened.push(ws),
            Err(e) => {
                eprintln!("couldn't reopen a window: {e:#}");
                failed = Some(e);
            }
        }
    }
    match failed {
        Some(e) if opened.is_empty() => Err(e),
        _ => Ok(opened),
    }
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
        let shell = cx.new(|_| CachedWorkspace(view.into()));
        cx.new(|cx| gpui_component::Root::new(shell, window, cx))
    })?;
    made.ok_or_else(|| anyhow::anyhow!("the window opened without its workspace"))
}

/// Root's child: the workspace as a cached view. A dialog in Root (the command palette) re-renders Root on every
/// keystroke and hover, which would re-render the workspace too; cached, it renders only on its own notify or a
/// window refresh (docs/gpui/redraw.md).
struct CachedWorkspace(gpui::AnyView);

impl gpui::Render for CachedWorkspace {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        use gpui::Styled as _;
        self.0
            .clone()
            .cached(gpui::StyleRefinement::default().size_full())
    }
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
    let quitting = reg.quitting;
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
    // Its queue loops stop now: a workspace kept alive past its window would still take (and drop) calls.
    if let Some(ws) = gone.workspace.upgrade().filter(|_| held) {
        ws.update(cx, |ws, _| ws.release_hub());
    }
    // Its connections and forwarded ports close with it, even if the workspace lives on a while.
    if let Some(ws) = gone.workspace.upgrade() {
        ws.update(cx, |ws, cx| ws.leave_machines(cx));
    }
    for view in &all {
        view.update(cx, |v, _| v.lists.remove(gone.window));
    }
    let orphans: Vec<_> = all
        .into_iter()
        .filter(|v| v.read(cx).window == gone.window)
        .collect();
    let Some((window, workspace)) = next else {
        return;
    };
    if !quitting {
        save_open_layouts(&workspace, cx);
    }
    let _ = window.update(cx, |_, window, cx| {
        for view in orphans {
            view.update(cx, |v, cx| v.attach(window, cx));
        }
        if held {
            let _ = workspace.update(cx, |ws, cx| ws.claim_hub(window, cx));
        }
    });
}

/// A window the user closed leaves the saved list: the windows still open are written now.
fn save_open_layouts(survivor: &WeakEntity<Workspace>, cx: &mut App) {
    let Some(path) = survivor
        .upgrade()
        .and_then(|ws| ws.read(cx).layout.path.clone())
    else {
        return;
    };
    let windows = open_layouts(cx);
    if windows.is_empty() {
        return;
    }
    cx.background_spawn(async move {
        if let Err(e) = super::layout::write_layouts(&path, windows) {
            eprintln!("couldn't save the window layout: {e:#}");
        }
    })
    .detach();
}

impl Workspace {
    /// Joins the registry: adopts every thread view already open on this store, and leaves it when the window closes.
    pub(super) fn register_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let me = Registered {
            window: window.window_handle(),
            workspace: cx.weak_entity(),
            store: self.backend.store.clone(),
            layout: None,
        };
        let reg = cx.default_global::<Windows>();
        reg.workspaces.push(me);
        let all = reg.threads.clone();
        let shared: Vec<_> = all
            .into_iter()
            .filter(|v| Arc::ptr_eq(&v.read(cx).backend.store, &self.backend.store))
            .collect();
        let open: Vec<_> = window_handles(cx);
        for view in shared {
            self.track_thread(&view, window, cx);
            // Its window closed with none left to take it (macOS keeps running): this one owns it now.
            if !open.contains(&view.read(cx).window) {
                view.update(cx, |v, cx| v.attach(window, cx));
            }
        }
        // Unsaved Preview edits ask before the window closes; off macOS closing the last window quits, so it asks
        // first like ⌘Q.
        window.on_window_should_close(cx, super::quit_guard::may_close);
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
        super::scheduled::start(self.backend.store.clone(), cx);
    }

    /// Stops serving the call queues (its window closed); the next window claims them.
    fn release_hub(&mut self) {
        self._orchestrate = None;
        if let Some(hook) = self.remote.as_mut() {
            hook._calls = None;
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
        super::orchestrate_ui::DelegatedStarts::forget(view.entity_id(), cx);
        if let Some(id) = view.read(cx).thread.as_ref().map(|t| t.id) {
            super::device_agent::DeviceClaims::release_thread(id, cx);
        }
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

    pub(super) fn other_windows(&self, cx: &App) -> Vec<(AnyWindowHandle, WeakEntity<Workspace>)> {
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

    /// Shows the view another window already has for thread `id` (its share may not have reached this window yet),
    /// rather than make a second one: one agent session per thread.
    /// Thread `id`'s view in any window of this store, when one is open.
    pub(super) fn shared_view(&self, id: i64, cx: &App) -> Option<Entity<ThreadView>> {
        cx.try_global::<Windows>().and_then(|reg| {
            reg.threads
                .iter()
                .find(|v| {
                    Arc::ptr_eq(&v.read(cx).backend.store, &self.backend.store)
                        && v.read(cx).thread.as_ref().map(|t| t.id) == Some(id)
                })
                .cloned()
        })
    }

    pub(super) fn show_shared(
        &mut self,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(view) = self.shared_view(id, cx) else {
            return false;
        };
        if !self.open_threads.contains(&view) {
            self.track_thread(&view, window, cx);
        }
        self.show(super::center::Page::Thread(view.downgrade()), window, cx);
        true
    }

    /// Whether another window shows thread `id` while it is the active window: the user is looking at it.
    pub(super) fn seen_elsewhere(&self, id: i64, cx: &App) -> bool {
        let active = cx.active_window();
        self.other_windows(cx).into_iter().any(|(window, ws)| {
            Some(window) == active
                && ws
                    .upgrade()
                    .is_some_and(|ws| ws.read(cx).shows_thread(id, cx))
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
