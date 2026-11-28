//! The dock engine: which panels each group holds and shows, which docks are open, zoom, the lock, and what panels
//! are told. Sizes are in `resize.rs`, drawing in `group.rs`.

use super::group::{DockAreaRenderer, TabGroupRenderer};
use super::resize::Dock;
use super::{DockPlacement, Panel, PanelId, PanelView, Skin};
use gpui::{App, Bounds, Context, Entity, EventEmitter, FocusHandle, Pixels, SharedString, Window};
use std::{collections::HashMap, rc::Rc, sync::Arc};

pub enum DockEvent {
    /// The panels, a shown tab, a dock's size or whether it is open changed.
    LayoutChanged,
}

/// The panels of one group, in tab order.
#[derive(Default)]
pub struct DockLayout(Vec<Arc<dyn PanelView>>);

impl DockLayout {
    pub fn tabs() -> Self {
        Self::default()
    }

    pub fn panel_view(mut self, panel: Arc<dyn PanelView>, _: &App) -> Self {
        self.0.push(panel);
        self
    }
}

/// A group's panels, in tab order, and which one is shown.
pub struct GroupState {
    pub panels: Vec<PanelId>,
    pub active_ix: usize,
}

pub(super) struct Group {
    pub panels: Vec<Arc<dyn PanelView>>,
    pub active_ix: usize,
    /// The last `set_active` each panel heard.
    told: HashMap<PanelId, bool>,
    /// Holds the focus of a group with no panel.
    pub focus: FocusHandle,
    /// One for the group's whole life: it holds the tab bar's scroll.
    pub renderer: Rc<dyn TabGroupRenderer>,
}

impl Group {
    pub fn active_panel(&self) -> Option<&Arc<dyn PanelView>> {
        self.panels.get(self.active_ix)
    }

    fn position(&self, id: PanelId) -> Option<usize> {
        self.panels.iter().position(|p| id_of(p) == id)
    }
}

const PLACEMENTS: [DockPlacement; 4] = [
    DockPlacement::Center,
    DockPlacement::Left,
    DockPlacement::Right,
    DockPlacement::Bottom,
];

pub struct DockArea {
    pub(super) id: SharedString,
    pub(super) renderer: Rc<dyn DockAreaRenderer>,
    /// Indexed by [`DockPlacement`].
    groups: [Group; 4],
    /// Indexed by [`DockPlacement`]; the center's stays `None`, the others' until something is docked there.
    pub(super) docks: [Option<Dock>; 4],
    pub(super) zoomed: Option<DockPlacement>,
    locked: bool,
    pub(super) bounds: Bounds<Pixels>,
    pub(super) focus: FocusHandle,
    sync_scheduled: bool,
}

impl EventEmitter<DockEvent> for DockArea {}

impl DockArea {
    pub fn new(id: impl Into<SharedString>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let renderer: Rc<dyn DockAreaRenderer> = Skin::new(cx);
        let groups = std::array::from_fn(|_| Group {
            panels: Vec::new(),
            active_ix: 0,
            told: HashMap::new(),
            focus: cx.focus_handle(),
            renderer: renderer.tab_group_renderer(),
        });
        Self {
            id: id.into(),
            renderer,
            groups,
            docks: [None; 4],
            zoomed: None,
            locked: false,
            bounds: Bounds::default(),
            focus: cx.focus_handle(),
            sync_scheduled: false,
        }
    }

    pub fn with_renderer(mut self, renderer: Rc<dyn DockAreaRenderer>) -> Self {
        for group in &mut self.groups {
            group.renderer = renderer.tab_group_renderer();
        }
        self.renderer = renderer;
        self
    }

    pub(super) fn group(&self, at: DockPlacement) -> &Group {
        &self.groups[at as usize]
    }

    /// The group at `at`; `None` where nothing was ever docked.
    pub fn tabs(&self, at: DockPlacement) -> Option<GroupState> {
        let there = at == DockPlacement::Center || self.docks[at as usize].is_some();
        there.then(|| GroupState {
            panels: self.group(at).panels.iter().map(id_of).collect(),
            active_ix: self.group(at).active_ix,
        })
    }

    pub fn panel(&self, id: PanelId) -> Option<&Arc<dyn PanelView>> {
        let mut panels = self.groups.iter().flat_map(|g| &g.panels);
        panels.find(|p| id_of(p) == id)
    }

    #[cfg(test)]
    pub fn is_empty(&self, at: DockPlacement, _: &App) -> bool {
        self.group(at).panels.is_empty()
    }

    /// A locked area closes no tab from its tab bars or [`super::ClosePanel`].
    pub fn set_locked(&mut self, locked: bool, _: &mut Window, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.locked, locked) != locked {
            cx.notify();
        }
    }

    pub fn set_center(&mut self, layout: DockLayout, window: &mut Window, cx: &mut Context<Self>) {
        self.set_dock(DockPlacement::Center, layout, window, cx);
    }

    /// Replaces the panels at `at`; a dock that wasn't there starts open.
    pub fn set_dock(
        &mut self,
        at: DockPlacement,
        layout: DockLayout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ensure_dock(at, None);
        let group = &mut self.groups[at as usize];
        let old = std::mem::replace(&mut group.panels, layout.0);
        group.active_ix = 0;
        let kept: Vec<_> = group.panels.iter().map(id_of).collect();
        self.changed(window, cx);
        for panel in old.iter().filter(|p| !kept.contains(&id_of(p))) {
            panel.on_removed(window, cx);
        }
    }

    fn ensure_dock(&mut self, at: DockPlacement, size: Option<Pixels>) {
        if at != DockPlacement::Center {
            self.docks[at as usize].get_or_insert_with(|| Dock::new(size));
        }
    }

    pub fn is_dock_open(&self, at: DockPlacement) -> bool {
        self.docks[at as usize].is_some_and(|d| d.open)
    }

    pub fn toggle_dock(&mut self, at: DockPlacement, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dock) = &mut self.docks[at as usize] {
            dock.open = !dock.open;
            self.changed(window, cx);
        }
    }

    /// Adds `panel` as the last tab at `at` and shows it there. Neither the focus nor the dock's open state changes;
    /// `size` is the size of a dock that wasn't there.
    pub fn add_panel_view(
        &mut self,
        panel: Arc<dyn PanelView>,
        at: DockPlacement,
        size: Option<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.panel(id_of(&panel)).is_some() {
            return self.select_panel(id_of(&panel), window, cx);
        }
        self.ensure_dock(at, size);
        let group = &mut self.groups[at as usize];
        group.panels.push(panel);
        group.active_ix = group.panels.len() - 1;
        self.changed(window, cx);
    }

    /// Shows `id`'s tab in its group, without moving the focus.
    pub fn select_panel(&mut self, id: PanelId, window: &mut Window, cx: &mut Context<Self>) {
        let Some((at, ix)) = self.find(id) else {
            return;
        };
        let group = &mut self.groups[at as usize];
        if group.active_ix != ix {
            group.active_ix = ix;
            self.changed(window, cx);
        }
    }

    /// A tab was clicked: shows it and focuses its panel.
    pub(super) fn select_tab(
        &mut self,
        at: DockPlacement,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let group = &mut self.groups[at as usize];
        if ix >= group.panels.len() || ix == group.active_ix {
            return;
        }
        group.active_ix = ix;
        group.panels[ix].focus_handle(cx).focus(window, cx);
        self.changed(window, cx);
    }

    pub fn remove_panel<P: Panel>(
        &mut self,
        panel: Entity<P>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.remove(panel.entity_id().into(), window, cx);
    }

    fn remove(&mut self, id: PanelId, window: &mut Window, cx: &mut Context<Self>) {
        let Some((at, ix)) = self.find(id) else {
            return;
        };
        let group = &mut self.groups[at as usize];
        let panel = group.panels.remove(ix);
        if ix < group.active_ix {
            group.active_ix -= 1;
        }
        group.active_ix = group.active_ix.min(group.panels.len().saturating_sub(1));
        let focused = window.focused(cx);
        let had_focus = panel.focus_handle(cx).contains_focused(window, cx);
        self.changed(window, cx);
        panel.on_removed(window, cx);
        // The focus follows to what shows next, unless the panel moved it itself.
        if !had_focus || window.focused(cx) != focused {
            return;
        }
        let next = std::iter::once(at)
            .chain(PLACEMENTS.into_iter().filter(|p| *p != at))
            .filter(|p| !self.is_collapsed(*p) && self.zoomed.is_none_or(|z| z == *p))
            .find_map(|p| self.group(p).active_panel());
        if let Some(next) = next {
            next.focus_handle(cx).focus(window, cx);
        }
    }

    /// Whether the group at `at` may close tabs: not while locked or zoomed, and never its last one.
    pub(super) fn may_close(&self, at: DockPlacement) -> bool {
        !self.locked && self.zoomed != Some(at) && self.group(at).panels.len() > 1
    }

    /// A close asked for from the group at `at`.
    pub(super) fn close(
        &mut self,
        at: DockPlacement,
        id: PanelId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let group = self.group(at);
        let closable = group
            .position(id)
            .is_some_and(|ix| group.panels[ix].closable(cx));
        if closable && self.may_close(at) {
            self.remove(id, window, cx);
        }
    }

    pub fn is_zoomed(&self) -> bool {
        self.zoomed.is_some()
    }

    pub fn set_zoomed_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(at) = self.zoomed.take() {
            self.tell_zoomed(at, false, window, cx);
            cx.notify();
        }
    }

    /// Zooming out always works; zooming in needs a zoomable shown tab.
    pub(super) fn toggle_zoom(
        &mut self,
        at: DockPlacement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.zoomed == Some(at) {
            return self.set_zoomed_out(window, cx);
        }
        if self
            .group(at)
            .active_panel()
            .is_some_and(|p| p.zoomable(cx))
        {
            self.set_zoomed_out(window, cx);
            self.zoomed = Some(at);
            self.tell_zoomed(at, true, window, cx);
            cx.notify();
        }
    }

    fn tell_zoomed(&self, at: DockPlacement, zoomed: bool, window: &Window, cx: &Context<Self>) {
        if let Some(panel) = self.group(at).active_panel().cloned() {
            cx.spawn_in(window, async move |_, cx| {
                _ = cx.update(|window, cx| panel.set_zoomed(zoomed, window, cx));
            })
            .detach();
        }
    }

    /// Whether the group at `at` is in a closed dock.
    pub(super) fn is_collapsed(&self, at: DockPlacement) -> bool {
        at != DockPlacement::Center && !self.is_dock_open(at)
    }

    fn find(&self, id: PanelId) -> Option<(DockPlacement, usize)> {
        PLACEMENTS
            .into_iter()
            .find_map(|at| Some((at, self.group(at).position(id)?)))
    }

    fn changed(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.schedule_active_sync(window, cx);
        cx.emit(DockEvent::LayoutChanged);
        cx.notify();
    }

    /// Tells panels what they became on the next tick, net of everything this update does: a native webview shown
    /// and hidden in one update would flash.
    pub(super) fn schedule_active_sync(&mut self, window: &Window, cx: &Context<Self>) {
        if std::mem::replace(&mut self.sync_scheduled, true) {
            return;
        }
        cx.spawn_in(window, async move |area, cx| {
            _ = cx.update(|window, cx| {
                let Ok(changes) = area.update(cx, |area, _| area.active_changes()) else {
                    return;
                };
                for (panel, active) in changes {
                    panel.set_active(active, window, cx);
                }
            });
        })
        .detach();
    }

    /// What each panel must hear now; in a group, the tab that hides before the one that shows.
    fn active_changes(&mut self) -> Vec<(Arc<dyn PanelView>, bool)> {
        self.sync_scheduled = false;
        let mut changes = Vec::new();
        for at in PLACEMENTS {
            let collapsed = self.is_collapsed(at);
            let group = &mut self.groups[at as usize];
            let ids: Vec<_> = group.panels.iter().map(id_of).collect();
            group.told.retain(|id, _| ids.contains(id));
            let mut shows = None;
            for (ix, panel) in group.panels.iter().enumerate() {
                let active = !collapsed && ix == group.active_ix;
                let was = group.told.insert(ids[ix], active);
                // A panel never shown doesn't hear that it is hidden.
                if was.unwrap_or(false) == active {
                    continue;
                }
                if active {
                    shows = Some((panel.clone(), true));
                } else {
                    changes.push((panel.clone(), false));
                }
            }
            changes.extend(shows);
        }
        changes
    }
}

fn id_of(panel: &Arc<dyn PanelView>) -> PanelId {
    panel.view().entity_id().into()
}
