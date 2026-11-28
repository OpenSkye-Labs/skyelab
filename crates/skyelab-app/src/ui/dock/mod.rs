//! SkyeLab's dock: panels, tab groups, the docks around the center, zoom and resize. The engine is gpui-base's
//! (`gpui_base::dock`) until stage 5b of the own UI kit; what the dock looks like, the panel trait and the "…" menu
//! are ours ([`Skin`]). The app names every dock item through this module, so the engine can change behind it.

mod handle;
mod skin;
mod tabs;

#[cfg(test)]
mod dock_parity_test;
#[cfg(test)]
mod dock_skin_test;

pub use gpui_base::dock::{
    CLOSED_BOTTOM_STRIP, DockArea, DockAreaRenderer, DockContext, DockEvent, DockLayout,
    DockPlacement, DropIndicator, NodeId, PaneRef, Panel as BasePanel, PanelEvent, PanelId,
    PanelState, PanelView as BasePanelView, TabGroupContext, TabGroupRenderer,
};
pub use skin::Skin;

use super::PopupMenu;
use gpui::{
    AnyElement, AnyView, App, Context, Entity, FocusHandle, IntoElement, SharedString, WeakEntity,
    Window,
};
use std::{any::Any, sync::Arc};

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

/// Where a panel offers zoom: the "…" menu's item, or that and a toolbar button.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum PanelControl {
    Both,
    #[default]
    Menu,
}

/// What a dock panel shows in its tab bar, on top of gpui-base's behavior trait.
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

/// The object-safe side of [`Panel`], for the tab bar.
trait PanelView: BasePanelView {
    fn tab_name(&self, cx: &App) -> Option<SharedString>;
    fn title(&self, window: &mut Window, cx: &mut App) -> AnyElement;
    fn title_suffix(&self, window: &mut Window, cx: &mut App) -> Option<AnyElement>;
    fn dropdown_menu(&self, menu: PopupMenu, window: &mut Window, cx: &mut App) -> PopupMenu;
    fn zoom_control(&self, cx: &App) -> Option<PanelControl>;
    fn title_bar(&self, cx: &App) -> bool;
}

impl<T: Panel> PanelView for Entity<T> {
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

/// The handle gpui-base holds for one of our panels. Concrete, so the skin gets our trait back from base's
/// `Arc<dyn PanelView>` with an `Any` downcast ([`PanelHandle::of`]).
#[derive(Clone)]
struct PanelHandle(Arc<dyn PanelView>);

impl PanelHandle {
    fn of(panel: &Arc<dyn BasePanelView>) -> Option<&Self> {
        panel.as_any().downcast_ref::<Self>()
    }
}

/// Wraps `panel` for [`DockLayout::panel_view`] and [`DockArea::add_panel_view`].
pub fn panel_handle<P: Panel>(panel: Entity<P>) -> Arc<dyn BasePanelView> {
    Arc::new(PanelHandle(Arc::new(panel)))
}

impl BasePanelView for PanelHandle {
    fn panel_name(&self, cx: &App) -> &'static str {
        self.0.panel_name(cx)
    }
    fn panel_id(&self, cx: &App) -> PanelId {
        self.0.panel_id(cx)
    }
    fn closable(&self, cx: &App) -> bool {
        self.0.closable(cx)
    }
    fn zoomable(&self, cx: &App) -> bool {
        self.0.zoomable(cx)
    }
    fn visible(&self, cx: &App) -> bool {
        self.0.visible(cx)
    }
    fn set_active(&self, active: bool, window: &mut Window, cx: &mut App) {
        self.0.set_active(active, window, cx)
    }
    fn set_zoomed(&self, zoomed: bool, window: &mut Window, cx: &mut App) {
        self.0.set_zoomed(zoomed, window, cx)
    }
    fn on_added_to(
        &self,
        group: WeakEntity<gpui_base::dock::TabGroup>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.0.on_added_to(group, window, cx)
    }
    fn on_removed(&self, window: &mut Window, cx: &mut App) {
        self.0.on_removed(window, cx)
    }
    fn view(&self) -> AnyView {
        self.0.view()
    }
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.0.focus_handle(cx)
    }
    fn dump(&self, cx: &App) -> PanelState {
        self.0.dump(cx)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A panel's title, or its name for a panel base holds without our handle.
fn panel_title(panel: &Arc<dyn BasePanelView>, window: &mut Window, cx: &mut App) -> AnyElement {
    match PanelHandle::of(panel) {
        Some(h) => h.0.title(window, cx),
        None => SharedString::from(panel.panel_name(cx)).into_any_element(),
    }
}

/// Where zoom goes for the group's shown panel: the panel must allow zooming at all (base refuses otherwise) and
/// say where the control appears.
fn zoom_control(group: &TabGroupContext, cx: &App) -> Option<PanelControl> {
    let panel = group.active_panel()?;
    panel
        .zoomable(cx)
        .then(|| PanelHandle::of(panel).and_then(|h| h.0.zoom_control(cx)))
        .flatten()
}
