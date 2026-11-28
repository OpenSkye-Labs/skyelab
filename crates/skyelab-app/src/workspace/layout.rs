//! The window's layout: the right tool dock (Changes, Browser, Devices), the page header with the dock toggles, and
//! saving the window frame, right dock and current page to settings.toml.

use super::{CONTEXT, ToggleBottomPanel, ToggleSidebar, Workspace};
use crate::ui::ActiveTheme as _;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use crate::ui::h_flex;
use gpui::{
    App, AppContext as _, Context, Div, Entity, Global, IntoElement, KeyBinding, NoAction, Pixels,
    Task, Window, actions, prelude::*, px,
};
use gpui_component::dock::{DockEvent, DockPlacement, Panel, panel_handle};
use skyelab_core::{AppSettings, WindowLayout};
use std::path::PathBuf;
use std::time::Duration;

actions!(
    skyelab,
    [
        ToggleRightPanel,
        ToggleThreadDetails,
        CloseTab,
        HideRightPanel
    ]
);

pub(crate) const TOGGLE_RIGHT_KEY: &str = if super::MAC {
    "cmd-alt-r"
} else {
    "ctrl-alt-r"
};
/// T3's thread details key.
/// T3's mod+w closes the right panel's active tab, never the window; a focused terminal's own binding wins.
pub(crate) const CLOSE_TAB_KEY: &str = if super::MAC { "cmd-w" } else { "ctrl-w" };
pub(crate) const TOGGLE_DETAILS_KEY: &str = if super::MAC {
    "cmd-shift-b"
} else {
    "ctrl-shift-b"
};
/// The right dock's width until the user resizes it.
pub(super) const RIGHT_DEFAULT_W: Pixels = px(560.);
/// A layout change is saved once it has been quiet this long (a resize drag fires on every frame).
const SAVE_AFTER: Duration = Duration::from_millis(600);

pub(super) fn bind_keys(cx: &mut App) {
    super::thread::composer::bind_keys(cx); // the thread composer's keys too: one call site in workspace::init
    cx.bind_keys([
        KeyBinding::new(TOGGLE_RIGHT_KEY, ToggleRightPanel, Some(CONTEXT)),
        KeyBinding::new(CLOSE_TAB_KEY, CloseTab, Some(CONTEXT)),
    ]);
    // Off macOS a ctrl- binding would steal the key from a focused shell (ctrl-w deletes a word).
    if !super::MAC {
        cx.bind_keys([
            KeyBinding::new(TOGGLE_RIGHT_KEY, NoAction, Some("Terminal")),
            KeyBinding::new(CLOSE_TAB_KEY, NoAction, Some("Terminal")),
        ]);
    }
}

/// Which docks are open, for the toggles' pressed state.
#[derive(Default, Clone, Copy)]
pub struct DockToggles {
    /// The floating thread details card.
    pub details: bool,
    pub left: bool,
    pub right: bool,
    pub bottom: bool,
}

/// Each window's [`DockToggles`]: windows have their own docks, and a thread view drawn in two windows shows each
/// one's toggles.
#[derive(Default)]
pub struct WindowDocks(pub std::collections::HashMap<gpui::WindowId, DockToggles>);

impl Global for WindowDocks {}

/// `window`'s dock toggles.
pub fn docks(window: &Window, cx: &App) -> DockToggles {
    docks_of(window.window_handle(), cx)
}

pub(crate) fn docks_of(window: gpui::AnyWindowHandle, cx: &App) -> DockToggles {
    cx.try_global::<WindowDocks>()
        .and_then(|d| d.0.get(&window.window_id()).copied())
        .unwrap_or_default()
}

/// The page's top row (T3's WorkspacePageHeader): 52 px, beside the sidebar's first row. With the sidebar hidden on
/// macOS the traffic lights sit over its start, so it leaves them room.
pub(crate) fn page_header(window: &Window, cx: &App) -> Div {
    let open = docks(window, cx);
    crate::ui::header(cx.theme())
        .when(super::MAC && !open.left, |h| {
            h.pl(crate::ui::MAC_TOP_PAD + gpui::px(8.))
        })
        // Windows: the caption buttons sit over the top row's end while nothing is docked right of the page.
        .when(crate::ui::window_controls::drawn() && !open.right, |h| {
            h.pr(crate::ui::window_controls::WINDOW_CONTROLS_W)
        })
}

pub(crate) fn panel_toggles(window: &Window, cx: &App) -> impl IntoElement {
    h_flex()
        .id("panel-toggles")
        .map(crate::ui::no_drag)
        .flex_shrink_0()
        .gap_0p5()
        .children(dock_toggles(docks(window, cx)))
}

fn dock_toggles(open: DockToggles) -> Vec<Button> {
    // The tooltip shows the action's key binding for this OS.
    let toggle = |id: &'static str,
                  icon: SkyeIcon,
                  on: bool,
                  tip: &'static str,
                  action: &dyn gpui::Action| {
        let action = action.boxed_clone();
        Button::new(id)
            .debug_selector(move || id.into())
            .ghost()
            .xsmall()
            .icon(icon)
            .toggled(on)
            .tooltip_with_action(tip, action.as_ref(), Some(CONTEXT))
            .on_click(move |_, window, cx| {
                crate::workspace::dispatch(action.boxed_clone(), window, cx)
            })
    };
    vec![
        toggle(
            "toggle-details",
            SkyeIcon::SquareMenu,
            open.details,
            "Toggle thread details",
            &ToggleThreadDetails,
        ),
        toggle(
            "toggle-sidebar",
            SkyeIcon::PanelLeft,
            open.left,
            "Toggle sidebar",
            &ToggleSidebar,
        ),
        toggle(
            "toggle-bottom",
            SkyeIcon::PanelBottom,
            open.bottom,
            "Toggle terminal",
            &ToggleBottomPanel,
        ),
        toggle(
            "toggle-right",
            SkyeIcon::PanelRight,
            open.right,
            "Toggle tools panel",
            &ToggleRightPanel,
        ),
    ]
}

/// What the hidden sidebar's edge handle drags (the width it had, for a drag that ends hidden); it draws nothing.
pub(super) struct RestoreSidebar(Pixels);

impl Render for RestoreSidebar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

/// T3's restore handle shows the sidebar once a drag from it is this far from the window's edge.
pub(super) const RESTORE_AFTER: Pixels = px(48.);

/// Where the layout is saved, this window's current layout, and the pending debounced save.
#[derive(Default)]
pub(super) struct LayoutStore {
    pub(super) path: Option<PathBuf>,
    save: Option<Task<()>>,
    /// A save is waiting; quitting writes at once instead.
    pending: bool,
    _quit: Option<gpui::Subscription>,
}

/// Writes `windows` as the list to reopen at launch (replacing an older file's single slot).
pub(super) fn write_layouts(
    path: &std::path::Path,
    windows: Vec<WindowLayout>,
) -> anyhow::Result<()> {
    AppSettings::update(path, |s| {
        s.windows = windows;
        s.window = None;
    })
    .map(|_| ())
}

impl Workspace {
    pub(super) fn toggle_right(
        &mut self,
        _: &ToggleRightPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle(DockPlacement::Right, window, cx);
    }

    /// Opens or closes the floating thread details card (T3's ⌘⇧B). It stays open while you work, until toggled
    /// or Esc; while it covers the chat column a click outside closes it too (`render::details_card`).
    pub(super) fn toggle_details(
        &mut self,
        _: &ToggleThreadDetails,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.details_open {
            self.close_details(window, cx);
        } else {
            self.set_details_open(true, cx);
        }
    }

    /// Closes the card. Focus inside it (the pull request field) goes back to the workspace, or the window's keys
    /// would reach nothing until a click.
    pub(super) fn close_details(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui::Focusable as _;
        if self.details.focus_handle(cx).contains_focused(window, cx) {
            self.focus.focus(window, cx);
        }
        self.set_details_open(false, cx);
    }

    pub(super) fn set_details_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.details_open != open {
            self.details_open = open;
            self.poll_usage(cx);
            // The toggles are drawn by the page header, a view of their own.
            self.sync_toggles(cx);
            self.center.update(cx, |_, cx| cx.notify());
            if let Some(view) = self.current_thread() {
                view.update(cx, |_, cx| cx.notify());
            }
        }
    }

    /// Adds a tool (Changes, a browser, Devices) to the right dock and opens the dock, so the thread keeps the center.
    pub(super) fn add_right_panel<P: Panel>(
        &mut self,
        panel: Entity<P>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dock.update(cx, |dock, cx| {
            dock.add_panel_view(panel_handle(panel), DockPlacement::Right, None, window, cx);
            if !dock.is_dock_open(DockPlacement::Right) {
                dock.toggle_dock(DockPlacement::Right, window, cx);
            }
        });
    }

    /// Shows a tool already in the right dock: opens the dock first, or the selection lands on a hidden view.
    pub(super) fn show_right_panel(
        &mut self,
        id: gpui_component::dock::PanelId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dock.update(cx, |dock, cx| {
            if !dock.is_dock_open(DockPlacement::Right) {
                dock.toggle_dock(DockPlacement::Right, window, cx);
            }
            dock.select_panel(id, window, cx);
        });
    }

    /// Whether `panel` is the shown tab of the dock at `at`, with that dock open.
    pub(super) fn dock_shows(
        &self,
        at: DockPlacement,
        panel: gpui_component::dock::PanelId,
        cx: &App,
    ) -> bool {
        let dock = self.dock.read(cx);
        let Some(tree) = dock.layout(at).filter(|_| dock.is_dock_open(at)) else {
            return false;
        };
        let node = tree.find_panel_node(panel).and_then(|n| tree.find_node(n));
        matches!(node.map(|n| n.kind()), Some(gpui_component::dock::PaneRef::Tabs { panels, active_ix }) if panels.get(active_ix) == Some(&panel))
    }

    /// A dock changed: refresh the toggles, and save the layout once things settle.
    pub(super) fn dock_changed(
        &mut self,
        e: &DockEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(e, DockEvent::LayoutChanged) {
            self.sync_toggles(cx);
            self.schedule_layout_save(window, cx);
        }
    }

    pub(super) fn sync_toggles(&self, cx: &mut Context<Self>) {
        let dock = self.dock.read(cx);
        let open = DockToggles {
            details: self.details_open,
            left: dock.is_dock_open(DockPlacement::Left),
            right: dock.is_dock_open(DockPlacement::Right),
            bottom: dock.is_dock_open(DockPlacement::Bottom),
        };
        let id = self.window_handle.window_id();
        cx.default_global::<WindowDocks>().0.insert(id, open);
        cx.notify();
    }

    /// Saves to `path` from now on and restores the right dock from `saved`. Only the app calls this, so tests never
    /// write the user's settings.toml.
    pub fn restore_layout(
        &mut self,
        path: Option<PathBuf>,
        saved: Option<&WindowLayout>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(saved) = saved {
            self.restore_page(&saved.view, window, cx);
            self.restore_terminals(saved.terminals.clone(), window, cx);
            self.dock.update(cx, |dock, cx| {
                if saved.right_panel_width > 0 {
                    let w = px(saved.right_panel_width as f32);
                    dock.set_dock_size(DockPlacement::Right, w, window, cx);
                }
                if saved.right_panel_open != dock.is_dock_open(DockPlacement::Right) {
                    dock.toggle_dock(DockPlacement::Right, window, cx);
                }
            });
            self.fit_right_dock(true, window, cx);
        }
        self.layout.path = path;
        // A change in the last moments before quitting would otherwise never be written.
        self.layout._quit = Some(cx.on_app_quit(|this, cx| {
            // Windows closing as the app quits stay in the list: they all reopen.
            super::windows::quitting(cx);
            let pending = std::mem::take(&mut this.layout.pending);
            let write = this.layout.path.clone().filter(|_| pending);
            let windows = super::windows::open_layouts(cx);
            async move {
                if let Some(path) = write {
                    let _ = write_layouts(&path, windows);
                }
            }
        }));
        self._window_bounds = Some(cx.observe_window_bounds(window, |this, window, cx| {
            this.fit_right_dock(true, window, cx);
            this.schedule_layout_save(window, cx)
        }));
        self.sync_toggles(cx);
        // A new window joins the saved list without waiting for its first move.
        self.schedule_layout_save(window, cx);
    }

    pub(super) fn schedule_layout_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.layout.path.clone() else {
            return;
        };
        if super::windows::is_hidden(self.window_handle, cx) {
            return; // the hidden hub's save would empty the saved list
        }
        let dock = self.dock.read(cx);
        let layout = skyelab_core::WindowLayout {
            view: self.page.key(cx),
            terminals: self.terminal_layouts(cx),
            ..crate::window_layout::to_layout(
                window.window_bounds().get_bounds(),
                dock.is_dock_open(DockPlacement::Right),
                dock.dock_size(DockPlacement::Right)
                    .unwrap_or(RIGHT_DEFAULT_W),
            )
        };
        super::windows::set_layout(self.window_handle, layout, cx);
        self.layout.pending = true;
        // A newer change replaces (cancels) the pending save: only the settled layout is written.
        self.layout.save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_AFTER).await;
            let Ok(windows) = this.update(cx, |this, cx| {
                this.layout.pending = false;
                super::windows::open_layouts(cx)
            }) else {
                return;
            };
            let saved = cx
                .background_spawn(async move { write_layouts(&path, windows) })
                .await;
            if let Err(e) = saved {
                eprintln!("couldn't save the window layout: {e:#}");
            }
        }));
    }
}

impl Workspace {
    pub(super) fn toggle(
        &mut self,
        at: DockPlacement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dock.read(cx).is_dock_open(at) {
            self.dock_skin.start_close(at, cx);
            // A zoomed group in a closed dock draws nothing and has no Zoom Out: end the zoom first.
            if self.dock.read(cx).is_zoomed() {
                self.dock
                    .update(cx, |dock, cx| dock.set_zoomed_out(window, cx));
            }
        }
        if at == DockPlacement::Left {
            // A toggle mid-drag ends the edge drag: its release listener is gone with the open sidebar.
            self.dock_skin.sidebar_drag.take();
        }
        self.dock
            .update(cx, |dock, cx| dock.toggle_dock(at, window, cx));
        self.fit_right_dock(false, window, cx);
    }

    pub(super) fn toggle_sidebar(
        &mut self,
        _: &ToggleSidebar,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle(DockPlacement::Left, window, cx);
    }

    /// T3's restore handle: with the sidebar hidden, a 16x64 px strip 8 px in from the window's left edge, centred
    /// down it, that shows a bar on hover. A click shows the sidebar at its last width; a drag shows it at the
    /// pointer ([`Self::restore_drag_move`]).
    pub(super) fn sidebar_restore_edge(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        if self.dock.read(cx).is_dock_open(DockPlacement::Left) {
            return None;
        }
        let bar = cx.theme().border;
        let width = self
            .dock
            .read(cx)
            .dock_size(DockPlacement::Left)
            .unwrap_or(gpui_base::PANEL_MIN_SIZE);
        Some(
            gpui::div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(8.))
                .flex()
                .items_center()
                .child(
                    gpui::div()
                        .id("sidebar-restore")
                        .role(gpui::accesskit::Role::Button)
                        .aria_label("Show sidebar")
                        .group("sidebar-restore")
                        .debug_selector(|| "sidebar-restore".into())
                        .w(px(16.))
                        .h(px(64.))
                        .cursor(gpui::CursorStyle::ResizeRight)
                        .child(
                            gpui::div()
                                .w(px(8.))
                                .h_full()
                                .rounded_md()
                                .group_hover("sidebar-restore", |s| s.bg(bar)),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle(DockPlacement::Left, window, cx)
                        }))
                        .on_drag(RestoreSidebar(width), |d, _, _, cx| {
                            let width = d.0;
                            cx.new(|_| RestoreSidebar(width))
                        }),
                ),
        )
    }

    /// A drag from the restore handle: past [`RESTORE_AFTER`] the sidebar shows at the pointer's width (clamped
    /// like a resize), back under it hides again at its old width; wherever it is on release stays. Instant, as T3's is mid-drag.
    pub(super) fn restore_drag_move(
        &mut self,
        e: &gpui::DragMoveEvent<RestoreSidebar>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let x = (e.event.position.x - e.bounds.left()).max(Pixels::ZERO);
        let show = x >= RESTORE_AFTER;
        let max = e.bounds.size.width
            - self.dock_width(DockPlacement::Right, cx)
            - gpui_base::PANEL_MIN_SIZE;
        self.dock.update(cx, |dock, cx| {
            if dock.is_dock_open(DockPlacement::Left) != show {
                dock.toggle_dock(DockPlacement::Left, window, cx);
            }
            // Hidden again: keep the width it had before the drag.
            let width = if show { x.min(max) } else { e.drag(cx).0 };
            dock.set_dock_size(DockPlacement::Left, width, window, cx);
        });
    }

    /// A drag of the sidebar's edge was released past `center_skin::HIDE_BELOW`: hide it the way the toggle does,
    /// and keep the width it had before the drag (base clamped it to the minimum), so it comes back at that width.
    pub(super) fn hide_sidebar_from_drag(
        &mut self,
        width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.dock.read(cx).is_dock_open(DockPlacement::Left) {
            return;
        }
        self.toggle(DockPlacement::Left, window, cx);
        self.dock.update(cx, |dock, cx| {
            dock.set_dock_size(DockPlacement::Left, width, window, cx)
        });
    }

    pub(super) fn toggle_bottom(
        &mut self,
        _: &ToggleBottomPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let opening = !self.dock.read(cx).is_dock_open(DockPlacement::Bottom);
        self.toggle(DockPlacement::Bottom, window, cx);
        // T3's ⌘J opens a shell, not an empty drawer.
        let restoring = !self.terms.restoring.is_empty();
        if opening && !restoring && !self.terminals.iter().any(|t| t.read(cx).docked) {
            self.new_terminal(&super::NewTerminal, window, cx);
        }
    }
}
