//! SkyeLab's dock: panels, tab groups, the docks around the center, zoom and resize. The engine is gpui-base's
//! (`gpui_base::dock`) until stage 5b of the own UI kit; the app names every dock item through this module, so the
//! engine can change behind it.

pub use gpui_base::dock::{
    CLOSED_BOTTOM_STRIP, DockArea, DockAreaRenderer, DockContext, DockEvent, DockLayout,
    DockPlacement, DropIndicator, NodeId, PaneRef, Panel as BasePanel, PanelEvent, PanelId,
    PanelState, PanelView as BasePanelView, TabGroupContext, TabGroupRenderer,
};
#[cfg(test)]
pub use gpui_component::dock::ToggleZoom;
pub use gpui_component::dock::{ClosePanel, DockSkin, Panel, PanelControl, panel_handle};
