//! The window's layout: the right tool dock (Changes, Browser, Devices), the dock toggles at the end of the center
//! tab bar, and saving the window frame and right dock to settings.toml.

use super::{CONTEXT, ToggleBottomPanel, ToggleSidebar, Workspace};
use gpui::{
    App, AppContext as _, Context, Entity, Global, KeyBinding, NoAction, Pixels, Task, Window,
    actions, px,
};
use gpui_component::{
    IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    dock::{DockEvent, DockPlacement, Panel, panel_handle},
};
use skyelab_core::{AppSettings, WindowLayout};
use std::path::PathBuf;
use std::time::Duration;

actions!(skyelab, [ToggleRightPanel]);

pub(crate) const TOGGLE_RIGHT_KEY: &str = if super::MAC {
    "cmd-alt-r"
} else {
    "ctrl-alt-r"
};
/// The right dock's width until the user resizes it.
pub(super) const RIGHT_DEFAULT_W: Pixels = px(480.);
/// A layout change is saved once it has been quiet this long (a resize drag fires on every frame).
const SAVE_AFTER: Duration = Duration::from_millis(600);

pub(super) fn bind_keys(cx: &mut App) {
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
    pub left: bool,
    pub right: bool,
    pub bottom: bool,
}

impl Global for DockToggles {}

/// The three toggles at the end of the center tab bar: sidebar, bottom drawer, right panel.
pub(crate) fn dock_toggles(cx: &App) -> Vec<Button> {
    let open = cx.try_global::<DockToggles>().copied().unwrap_or_default();
    let toggle = |id: &'static str, icon: IconName, on: bool, tip: &'static str| {
        Button::new(id)
            .ghost()
            .xsmall()
            .icon(icon)
            .toggled(on)
            .tooltip(tip)
    };
    vec![
        toggle(
            "toggle-sidebar",
            IconName::PanelLeft,
            open.left,
            "Toggle sidebar",
        )
        .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleSidebar), cx)),
        toggle(
            "toggle-bottom",
            IconName::PanelBottom,
            open.bottom,
            "Toggle terminal",
        )
        .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleBottomPanel), cx)),
        toggle(
            "toggle-right",
            IconName::PanelRight,
            open.right,
            "Toggle tools panel",
        )
        .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleRightPanel), cx)),
    ]
}

/// Where the layout is saved, and the pending debounced save.
#[derive(Default)]
pub(super) struct LayoutStore {
    path: Option<PathBuf>,
    save: Option<Task<()>>,
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

    /// Adds a tool (Changes, a browser, Devices) to the right dock and opens the dock, so the thread keeps the center.
    pub(super) fn add_right_panel<P: Panel>(
        &mut self,
        panel: Entity<P>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let empty = self.right_empty.take();
        self.dock.update(cx, |dock, cx| {
            dock.add_panel_view(panel_handle(panel), DockPlacement::Right, None, window, cx);
            // Added before the placeholder leaves, so the dock never goes empty.
            if let Some(empty) = empty {
                dock.remove_panel(empty, window, cx);
            }
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
            left: dock.is_dock_open(DockPlacement::Left),
            right: dock.is_dock_open(DockPlacement::Right),
            bottom: dock.is_dock_open(DockPlacement::Bottom),
        };
        cx.set_global(open);
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
        self._window_bounds = Some(cx.observe_window_bounds(window, |this, window, cx| {
            this.schedule_layout_save(window, cx)
        }));
        self.sync_toggles(cx);
    }

    fn schedule_layout_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.layout.path.clone() else {
            return;
        };
        let dock = self.dock.read(cx);
        let layout = crate::window_layout::to_layout(
            window.window_bounds().get_bounds(),
            dock.is_dock_open(DockPlacement::Right),
            dock.dock_size(DockPlacement::Right)
                .unwrap_or(RIGHT_DEFAULT_W),
        );
        // A newer change replaces (cancels) the pending save: only the settled layout is written.
        self.layout.save = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(SAVE_AFTER).await;
            let saved = cx
                .background_spawn(async move {
                    AppSettings::update(&path, |s| s.window = Some(layout)).map(|_| ())
                })
                .await;
            if let Err(e) = saved {
                eprintln!("couldn't save the window layout: {e:#}");
            }
        }));
    }
}
