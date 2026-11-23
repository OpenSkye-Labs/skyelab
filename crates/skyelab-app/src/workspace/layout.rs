//! The window's layout: the right tool dock (Changes, Browser, Devices), the page header with the dock toggles, and
//! saving the window frame, right dock and current page to settings.toml.

use super::{CONTEXT, ToggleBottomPanel, ToggleSidebar, Workspace};
use gpui::{
    App, AppContext as _, Context, Div, Entity, Global, IntoElement, KeyBinding, NoAction, Pixels,
    Task, Window, actions, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    dock::{DockEvent, DockPlacement, Panel, panel_handle},
    h_flex,
};
use gpui_kit_assets::IconName;
use skyelab_core::{AppSettings, WindowLayout};
use std::path::PathBuf;
use std::time::Duration;

actions!(skyelab, [ToggleRightPanel, ToggleThreadDetails]);

pub(crate) const TOGGLE_RIGHT_KEY: &str = if super::MAC {
    "cmd-alt-r"
} else {
    "ctrl-alt-r"
};
/// T3's thread details key.
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
    cx.bind_keys([KeyBinding::new(
        TOGGLE_RIGHT_KEY,
        ToggleRightPanel,
        Some(CONTEXT),
    )]);
    // Off macOS a ctrl- binding would steal the key from a focused shell.
    if !super::MAC {
        cx.bind_keys([KeyBinding::new(
            TOGGLE_RIGHT_KEY,
            NoAction,
            Some("Terminal"),
        )]);
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
    crate::ui::header(cx.theme()).when(super::MAC && !open.left, |h| {
        h.pl(crate::ui::TRAFFIC_LIGHTS_W)
    })
}

pub(crate) fn panel_toggles(window: &Window, cx: &App) -> impl IntoElement {
    h_flex()
        .flex_shrink_0()
        .gap_0p5()
        .children(dock_toggles(docks(window, cx)))
}

fn dock_toggles(open: DockToggles) -> Vec<Button> {
    // The tooltip shows the action's key binding for this OS.
    let toggle = |id: &'static str,
                  icon: IconName,
                  on: bool,
                  tip: &'static str,
                  action: &dyn gpui::Action| {
        let action = action.boxed_clone();
        Button::new(id)
            .ghost()
            .xsmall()
            .icon(icon)
            .toggled(on)
            .tooltip_with_action(tip, action.as_ref(), Some(CONTEXT))
            .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
    };
    vec![
        toggle(
            "toggle-details",
            IconName::SquareMenu,
            open.details,
            "Toggle thread details",
            &ToggleThreadDetails,
        ),
        toggle(
            "toggle-sidebar",
            IconName::PanelLeft,
            open.left,
            "Toggle sidebar",
            &ToggleSidebar,
        ),
        toggle(
            "toggle-bottom",
            IconName::PanelBottom,
            open.bottom,
            "Toggle terminal",
            &ToggleBottomPanel,
        ),
        toggle(
            "toggle-right",
            IconName::PanelRight,
            open.right,
            "Toggle tools panel",
            &ToggleRightPanel,
        ),
    ]
}

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
            self.dock.update(cx, |dock, cx| {
                if saved.right_panel_width > 0 {
                    let w = px(saved.right_panel_width as f32);
                    dock.set_dock_size(DockPlacement::Right, w, window, cx);
                }
                if saved.right_panel_open != dock.is_dock_open(DockPlacement::Right) {
                    dock.toggle_dock(DockPlacement::Right, window, cx);
                }
            });
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
        let dock = self.dock.read(cx);
        let layout = skyelab_core::WindowLayout {
            view: self.page.key(cx),
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
        self.dock
            .update(cx, |dock, cx| dock.toggle_dock(at, window, cx));
    }

    pub(super) fn toggle_sidebar(
        &mut self,
        _: &ToggleSidebar,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle(DockPlacement::Left, window, cx);
    }

    pub(super) fn toggle_bottom(
        &mut self,
        _: &ToggleBottomPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle(DockPlacement::Bottom, window, cx);
    }
}
