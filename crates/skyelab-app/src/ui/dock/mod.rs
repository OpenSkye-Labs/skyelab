//! SkyeLab's dock: a center and a left, right and bottom dock, one tab group each, nothing dragged between them.
//! [`DockArea`] is the engine (`area.rs`, `resize.rs`), `group.rs` draws a group through the renderer traits, and
//! [`Skin`] is the look.

mod area;
mod group;
mod handle;
mod resize;
mod skin;
mod tabs;

#[cfg(test)]
mod dock_engine_parity_test;
#[cfg(test)]
mod dock_engine_resize_test;
#[cfg(test)]
mod dock_engine_test;
#[cfg(test)]
mod dock_redraw_test;
#[cfg(test)]
mod dock_skin_test;

pub use area::{DockArea, DockEvent, DockLayout};
pub use group::{DockAreaRenderer, TabGroupContext, TabGroupRenderer};
pub use resize::{CLOSED_BOTTOM_STRIP, DockContext, PANEL_MIN_SIZE};
pub use skin::Skin;

use super::PopupMenu;
use gpui::{
    AnyElement, AnyView, App, Axis, Context, Entity, EntityId, FocusHandle, Focusable, IntoElement,
    Render, SharedString, Window,
};
use std::sync::Arc;

gpui::actions!(
    skyelab_dock,
    [
        /// Fills the window with the focused panel's group, or puts it back; a panel offering no zoom control isn't
        /// zoomed in.
        ToggleZoom,
        /// Closes the focused group's shown tab, when the group allows it.
        ClosePanel
    ]
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DockPlacement {
    Center,
    Left,
    Bottom,
    Right,
}

impl DockPlacement {
    pub fn axis(self) -> Axis {
        match self {
            Self::Bottom => Axis::Vertical,
            _ => Axis::Horizontal,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PanelId(u64);

impl PanelId {
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl From<EntityId> for PanelId {
    fn from(id: EntityId) -> Self {
        Self(id.as_u64())
    }
}

/// Where a panel offers zoom: the "…" menu's item, or that and a toolbar button.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum PanelControl {
    Both,
    #[default]
    Menu,
}

pub trait BasePanel: Render + Focusable {
    fn panel_name(&self) -> &'static str;

    /// Whether its tab may be closed; the group can still refuse (locked, zoomed, its last tab).
    fn closable(&self, _: &App) -> bool {
        true
    }

    fn zoomable(&self, _: &App) -> bool {
        true
    }

    /// It became, or stopped being, the shown tab of a group whose dock is open (another group's zoom doesn't count):
    /// once per change, on the next tick. A removed panel hears [`Self::on_removed`] instead.
    fn set_active(&mut self, _active: bool, _: &mut Window, _: &mut Context<Self>) {}

    /// Its group zoomed in or out while it was the shown tab.
    fn set_zoomed(&mut self, _zoomed: bool, _: &mut Window, _: &mut Context<Self>) {}

    /// It left the dock. A focus move made here is kept.
    fn on_removed(&mut self, _: &mut Window, _: &mut Context<Self>) {}
}

/// What a dock panel shows in its tab bar.
pub trait Panel: BasePanel {
    /// The tab's accessible name (the title's text); the panel name when `None`.
    fn tab_name(&self, _: &App) -> Option<SharedString> {
        None
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        SharedString::from(self.panel_name())
    }

    /// Drawn after the title in a one-panel bar, before the toolbar in a tab bar.
    fn title_suffix(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<impl IntoElement> {
        None::<gpui::Div>
    }

    /// The panel's own items at the top of the "…" menu.
    fn dropdown_menu(
        &mut self,
        menu: PopupMenu,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> PopupMenu {
        menu
    }

    /// Where zoom is offered; `None` offers it nowhere.
    fn zoom_control(&self, _: &App) -> Option<PanelControl> {
        Some(PanelControl::Menu)
    }

    /// `false` for a panel that draws its own chrome: alone in its group, it gets no title bar.
    fn title_bar(&self, _: &App) -> bool {
        true
    }
}

/// The object-safe side of [`BasePanel`] and [`Panel`].
pub trait PanelView: 'static + Send + Sync {
    fn panel_name(&self, cx: &App) -> &'static str;
    fn panel_id(&self, cx: &App) -> PanelId;
    fn closable(&self, cx: &App) -> bool;
    fn zoomable(&self, cx: &App) -> bool;
    fn set_active(&self, active: bool, window: &mut Window, cx: &mut App);
    fn set_zoomed(&self, zoomed: bool, window: &mut Window, cx: &mut App);
    fn on_removed(&self, window: &mut Window, cx: &mut App);
    fn view(&self) -> AnyView;
    fn focus_handle(&self, cx: &App) -> FocusHandle;
    fn tab_name(&self, cx: &App) -> Option<SharedString>;
    fn title(&self, window: &mut Window, cx: &mut App) -> AnyElement;
    fn title_suffix(&self, window: &mut Window, cx: &mut App) -> Option<AnyElement>;
    fn dropdown_menu(&self, menu: PopupMenu, window: &mut Window, cx: &mut App) -> PopupMenu;
    fn zoom_control(&self, cx: &App) -> Option<PanelControl>;
    fn title_bar(&self, cx: &App) -> bool;
}

impl<T: Panel> PanelView for Entity<T> {
    fn panel_name(&self, cx: &App) -> &'static str {
        self.read(cx).panel_name()
    }
    fn panel_id(&self, _: &App) -> PanelId {
        self.entity_id().into()
    }
    fn closable(&self, cx: &App) -> bool {
        self.read(cx).closable(cx)
    }
    fn zoomable(&self, cx: &App) -> bool {
        self.read(cx).zoomable(cx)
    }
    fn set_active(&self, active: bool, window: &mut Window, cx: &mut App) {
        self.update(cx, |p, cx| p.set_active(active, window, cx))
    }
    fn set_zoomed(&self, zoomed: bool, window: &mut Window, cx: &mut App) {
        self.update(cx, |p, cx| p.set_zoomed(zoomed, window, cx))
    }
    fn on_removed(&self, window: &mut Window, cx: &mut App) {
        self.update(cx, |p, cx| p.on_removed(window, cx))
    }
    fn view(&self) -> AnyView {
        self.clone().into()
    }
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.read(cx).focus_handle(cx)
    }
    fn tab_name(&self, cx: &App) -> Option<SharedString> {
        self.read(cx).tab_name(cx)
    }
    fn title(&self, window: &mut Window, cx: &mut App) -> AnyElement {
        self.update(cx, |p, cx| p.title(window, cx).into_any_element())
    }
    fn title_suffix(&self, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        self.update(cx, |p, cx| {
            p.title_suffix(window, cx)
                .map(IntoElement::into_any_element)
        })
    }
    fn dropdown_menu(&self, menu: PopupMenu, window: &mut Window, cx: &mut App) -> PopupMenu {
        self.update(cx, |p, cx| p.dropdown_menu(menu, window, cx))
    }
    fn zoom_control(&self, cx: &App) -> Option<PanelControl> {
        self.read(cx).zoom_control(cx)
    }
    fn title_bar(&self, cx: &App) -> bool {
        self.read(cx).title_bar(cx)
    }
}

/// Wraps `panel` for [`DockLayout::panel_view`] and [`DockArea::add_panel_view`].
pub fn panel_handle<P: Panel>(panel: Entity<P>) -> Arc<dyn PanelView> {
    Arc::new(panel)
}

fn zoom_control(group: &TabGroupContext, cx: &App) -> Option<PanelControl> {
    let panel = group.active_panel()?;
    panel.zoomable(cx).then(|| panel.zoom_control(cx)).flatten()
}
