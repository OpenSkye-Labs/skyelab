//! Stage 5b acceptance: our dock engine (`ui::dock::DockArea`), panels and zoom. Every expected value here was
//! measured on gpui-base's engine at planning (main `8147abe11`), so the engine swap changes none of them. Resizes and
//! events are in `dock_engine_resize_test.rs`.

use crate::ui::dock::{
    self as dock, BasePanel, ClosePanel, DockArea, DockEvent, DockLayout, DockPlacement,
    PanelControl, ToggleZoom,
};
use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, Window, div, px,
    size,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub(super) const RIGHT: DockPlacement = DockPlacement::Right;

pub(super) struct Fix {
    name: &'static str,
    zoom: bool,
    closable: bool,
    log: Rc<RefCell<Vec<String>>>,
    /// Focused by `on_removed`, as a panel that hands the keys somewhere itself.
    focus_on_removed: Option<FocusHandle>,
    pub focus: FocusHandle,
}

impl Focusable for Fix {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Fix {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let name = self.name;
        div()
            .size_full()
            .track_focus(&self.focus)
            .debug_selector(move || format!("content-{name}"))
            .child(name)
    }
}

impl BasePanel for Fix {
    fn panel_name(&self) -> &'static str {
        self.name
    }
    fn closable(&self, _: &gpui::App) -> bool {
        self.closable
    }
    fn zoomable(&self, _: &gpui::App) -> bool {
        self.zoom
    }
    fn set_active(&mut self, active: bool, _: &mut Window, _: &mut Context<Self>) {
        self.log
            .borrow_mut()
            .push(format!("{} active {active}", self.name));
    }
    fn set_zoomed(&mut self, zoomed: bool, _: &mut Window, _: &mut Context<Self>) {
        self.log
            .borrow_mut()
            .push(format!("{} zoomed {zoomed}", self.name));
    }
    fn on_removed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.log.borrow_mut().push(format!("{} removed", self.name));
        if let Some(to) = &self.focus_on_removed {
            to.focus(window, cx);
        }
    }
}

impl dock::Panel for Fix {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.name
    }
    fn zoom_control(&self, _: &gpui::App) -> Option<PanelControl> {
        self.zoom.then_some(PanelControl::Both)
    }
    fn title_bar(&self, _: &gpui::App) -> bool {
        self.name != "Center"
    }
}

/// The area as the workspace places it: beside a 48 px rail and over a 32 px status bar, so it is not the window.
struct Host(Entity<DockArea>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .pl(px(48.))
            .pb(px(32.))
            .child(self.0.clone())
    }
}

/// The group at `at`: its panels' ids and the shown index.
fn tabs(area: &DockArea, at: DockPlacement, cx: &gpui::App) -> Option<(Vec<dock::PanelId>, usize)> {
    let _ = cx;
    area.tabs(at).map(|t| (t.panels, t.active_ix))
}

pub(super) struct Rig<'a> {
    pub area: Entity<DockArea>,
    log: Rc<RefCell<Vec<String>>>,
    events: Rc<Cell<usize>>,
    pub center: Entity<Fix>,
    pub vcx: &'a mut VisualTestContext,
}

impl Rig<'_> {
    pub fn fix(&mut self, name: &'static str, zoom: bool, closable: bool) -> Entity<Fix> {
        let log = self.log.clone();
        self.vcx.update(|_, cx| {
            cx.new(|cx| Fix {
                name,
                zoom,
                closable,
                log,
                focus_on_removed: None,
                focus: cx.focus_handle(),
            })
        })
    }

    /// Runs `f` on the area, then everything it scheduled; returns what the panels were told.
    pub fn act(
        &mut self,
        f: impl FnOnce(&mut DockArea, &mut Window, &mut Context<DockArea>),
    ) -> Vec<String> {
        self.area.update_in(self.vcx, f);
        self.drain()
    }

    pub fn drain(&mut self) -> Vec<String> {
        self.vcx.run_until_parked();
        std::mem::take(&mut *self.log.borrow_mut())
    }

    /// `LayoutChanged` events since the last call.
    pub fn events(&mut self) -> usize {
        self.events.take()
    }

    pub fn dock(&mut self, at: DockPlacement, panels: &[&Entity<Fix>], size: f32) -> Vec<String> {
        let panels: Vec<_> = panels.iter().map(|p| (*p).clone()).collect();
        self.act(|a, window, cx| {
            let layout = panels.into_iter().fold(DockLayout::tabs(), |l, p| {
                l.panel_view(dock::panel_handle(p), cx)
            });
            a.set_dock(at, layout, window, cx);
            a.set_dock_size(at, px(size), window, cx);
        })
    }

    pub fn add(&mut self, at: DockPlacement, panel: &Entity<Fix>) -> Vec<String> {
        let panel = dock::panel_handle(panel.clone());
        self.act(|a, window, cx| a.add_panel_view(panel, at, None, window, cx))
    }

    pub fn remove(&mut self, panel: &Entity<Fix>) -> Vec<String> {
        let panel = panel.clone();
        self.act(|a, window, cx| a.remove_panel(panel, window, cx))
    }

    pub fn select(&mut self, panel: &Entity<Fix>) -> Vec<String> {
        let id = panel.entity_id().into();
        self.act(|a, window, cx| a.select_panel(id, window, cx))
    }

    pub fn toggle(&mut self, at: DockPlacement) -> Vec<String> {
        self.act(|a, window, cx| a.toggle_dock(at, window, cx))
    }

    pub fn focus(&mut self, panel: &Entity<Fix>) {
        panel.update_in(self.vcx, |p, window, cx| p.focus.focus(window, cx));
        self.vcx.run_until_parked();
    }

    pub fn focused(&mut self, panel: &Entity<Fix>) -> bool {
        panel.update_in(self.vcx, |p, window, _| p.focus.is_focused(window))
    }

    /// How many tabs the group at `at` has, and which is shown.
    pub fn tabs(&mut self, at: DockPlacement) -> (usize, usize) {
        let found = self.area.read_with(self.vcx, |a, cx| tabs(a, at, cx));
        let (panels, ix) = found.expect("a group there");
        (panels.len(), ix)
    }

    pub fn drawn(&mut self, selector: &'static str) -> bool {
        self.vcx.debug_bounds(selector).is_some()
    }

    fn zoomed(&mut self) -> bool {
        self.area.read_with(self.vcx, |a, _| a.is_zoomed())
    }

    fn zoom_from(&mut self, panel: &Entity<Fix>) -> Vec<String> {
        self.focus(panel);
        self.vcx.dispatch_action(ToggleZoom);
        self.drain()
    }

    fn close_from(&mut self, panel: &Entity<Fix>) -> Vec<String> {
        self.focus(panel);
        self.vcx.dispatch_action(ClosePanel);
        self.drain()
    }
}

/// A 1000x700 dock area at (48, 0) in a 1048x732 window, with a center panel and nothing docked yet.
pub(super) fn rig(cx: &mut TestAppContext) -> Rig<'_> {
    super::super::button_test::setup(cx);
    let mut built = None;
    let (_host, vcx) = cx.add_window_view(|window, cx| {
        window.set_rem_size(crate::ui::ActiveTheme::theme(&**cx).font_size);
        let area = cx
            .new(|cx| DockArea::new("engine-test", window, cx).with_renderer(dock::Skin::new(cx)));
        built = Some(area.clone());
        Host(area)
    });
    let area = built.expect("the window view was built");
    vcx.simulate_resize(size(px(1048.), px(732.)));
    let events = Rc::new(Cell::new(0));
    let counted = events.clone();
    vcx.update(|_, cx| {
        cx.subscribe(&area, move |_, e: &DockEvent, _| {
            if matches!(e, DockEvent::LayoutChanged) {
                counted.set(counted.get() + 1);
            }
        })
        .detach();
    });
    let log = Rc::<RefCell<Vec<String>>>::default();
    let center = vcx.update(|_, cx| {
        let log = log.clone();
        cx.new(|cx| Fix {
            name: "Center",
            zoom: false,
            closable: false,
            log,
            focus_on_removed: None,
            focus: cx.focus_handle(),
        })
    });
    let mut rig = Rig {
        area,
        log,
        events,
        center: center.clone(),
        vcx,
    };
    rig.act(|a, window, cx| {
        let layout = DockLayout::tabs().panel_view(dock::panel_handle(center), cx);
        a.set_center(layout, window, cx);
    });
    rig.events();
    rig
}

#[gpui::test]
fn panels_hear_each_active_edge_once_on_the_next_tick(cx: &mut TestAppContext) {
    let mut rig = rig(cx);
    let (a, b) = (rig.fix("A", false, true), rig.fix("B", false, true));
    assert_eq!(rig.dock(RIGHT, &[&a], 300.), ["A active true"]);
    let (log, handle) = (rig.log.clone(), dock::panel_handle(b.clone()));
    let told = rig.act(|area, window, cx| {
        area.add_panel_view(handle, RIGHT, None, window, cx);
        assert!(log.borrow().is_empty(), "nothing before the next tick");
    });
    assert_eq!(
        told,
        ["A active false", "B active true"],
        "an added tab shows"
    );
    assert_eq!(rig.tabs(RIGHT), (2, 1));
    assert_eq!(
        rig.select(&b),
        [""; 0],
        "selecting the shown tab says nothing"
    );
    assert_eq!(
        rig.toggle(RIGHT),
        ["B active false"],
        "a closing dock hides it"
    );
    assert_eq!(rig.toggle(RIGHT), ["B active true"]);

    // Closed, then opened and switched in one update (`show_right_panel`): B, never on screen, hears nothing.
    rig.toggle(RIGHT);
    let id = a.entity_id().into();
    let told = rig.act(|area, window, cx| {
        area.toggle_dock(RIGHT, window, cx);
        area.select_panel(id, window, cx);
    });
    assert_eq!(told, ["A active true"]);

    // A tab added to a closed dock waits for the dock to open.
    let c = rig.fix("C", false, true);
    rig.toggle(RIGHT);
    assert_eq!(rig.add(RIGHT, &c), [""; 0]);
    assert!(!rig.area.read_with(rig.vcx, |a, _| a.is_dock_open(RIGHT)));
    assert_eq!(rig.toggle(RIGHT), ["C active true"]);
}

#[gpui::test]
fn a_removed_tab_hands_over_to_the_right_one(cx: &mut TestAppContext) {
    let mut rig = rig(cx);
    let [a, b, c, d] = ["A", "B", "C", "D"].map(|n| rig.fix(n, false, true));
    rig.dock(RIGHT, &[&a, &b, &c, &d], 300.);
    rig.select(&b);
    assert_eq!(
        rig.remove(&b),
        ["B removed", "C active true"],
        "the next tab shows"
    );
    assert!(rig.drawn("content-C"));
    assert_eq!(
        rig.remove(&a),
        ["A removed"],
        "an earlier tab leaves C shown"
    );
    assert_eq!(rig.tabs(RIGHT), (2, 0));
    assert!(rig.drawn("content-C"));
    rig.select(&d);
    assert_eq!(
        rig.remove(&d),
        ["D removed", "C active true"],
        "the last tab: the one before"
    );
}

#[gpui::test]
fn the_focus_of_a_removed_tab_goes_to_what_shows_next(cx: &mut TestAppContext) {
    let mut rig = rig(cx);
    let [a, b, c, d] = ["A", "B", "C", "D"].map(|n| rig.fix(n, false, true));
    rig.dock(RIGHT, &[&a, &b, &c, &d], 300.);
    let center = rig.center.clone();
    rig.focus(&center);
    rig.select(&b);
    assert!(rig.focused(&center), "select_panel moves no focus");
    // An agent opening a browser tab while the user types must not take the keys.
    let [e, f] = ["E", "F"].map(|n| rig.fix(n, false, true));
    rig.add(RIGHT, &e);
    assert!(rig.focused(&center), "nor does adding a tab");
    rig.toggle(RIGHT);
    rig.add(RIGHT, &f);
    rig.toggle(RIGHT);
    assert!(rig.focused(&center), "nor closing and opening a dock");
    rig.remove(&e);
    rig.remove(&f);
    rig.select(&b);
    rig.remove(&b);
    assert!(rig.focused(&center), "nor does removing a tab without it");
    rig.focus(&c);
    rig.close_from(&c);
    assert!(
        rig.focused(&d),
        "ClosePanel hands the focus to the tab that shows"
    );
    // A panel that moves the focus itself as it leaves keeps its choice.
    let to = center.read_with(rig.vcx, |c, _| c.focus.clone());
    d.update(rig.vcx, |d, _| d.focus_on_removed = Some(to));
    rig.remove(&d);
    assert!(rig.focused(&center), "on_removed's focus move is kept");
    rig.focus(&a);
    rig.remove(&a);
    assert_eq!(rig.tabs(RIGHT).0, 0);
    assert!(
        rig.focused(&center),
        "an emptied group: the center takes it"
    );
}

#[gpui::test]
fn each_group_keeps_its_own_tab_bar_scroll_across_redraws(cx: &mut TestAppContext) {
    let mut rig = rig(cx);
    let right = ["A", "B", "C", "D", "E", "F", "G", "H"].map(|n| rig.fix(n, false, true));
    rig.dock(RIGHT, &right.iter().collect::<Vec<_>>(), 300.);
    let left = |rig: &mut Rig<'_>, s| rig.vcx.debug_bounds(s).expect("the tab is laid out").left();
    let start = left(&mut rig, "dock-tab-0");
    rig.select(&right[7]);
    rig.act(|_, _, cx| cx.notify());
    let last = rig.vcx.debug_bounds("dock-tab-7").expect("the last tab");
    assert!(
        last.right() <= px(1048.),
        "the selected tab scrolls into view"
    );
    assert!(left(&mut rig, "dock-tab-0") < start);

    // The user wheels the bar back; a redraw (a streamed delta) must not scroll it to the shown tab again.
    rig.vcx.simulate_event(gpui::ScrollEvent {
        position: gpui::point(px(800.), px(16.)),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(2000.), px(0.))),
        ..Default::default()
    });
    rig.drain();
    assert_eq!(left(&mut rig, "dock-tab-0"), start, "wheeled back");
    rig.act(|_, _, cx| cx.notify());
    rig.act(|_, _, cx| cx.notify());
    assert_eq!(left(&mut rig, "dock-tab-0"), start, "and it stays there");

    // The drawer's bar has its own scroll: it overflows too, and a wheel over the right bar moves only the right one.
    // Both draw a `dock-tab-0`, so the tabs are found by name.
    let mut bottom = vec![rig.fix("Drawer", false, true)];
    bottom.extend((0..24).map(|_| rig.fix("T", false, true)));
    rig.dock(
        DockPlacement::Bottom,
        &bottom.iter().collect::<Vec<_>>(),
        200.,
    );
    let named = |rig: &mut Rig<'_>, name: &str| {
        let all = rig
            .vcx
            .update(|window, _| gpui_base::test_support::snapshots(window));
        let tab = all
            .iter()
            .find(|s| s.role() == Some(gpui::Role::Tab) && s.label() == Some(name));
        tab.expect("the tab is laid out").bounds().left()
    };
    let (right_at, drawer_at) = (named(&mut rig, "A"), named(&mut rig, "Drawer"));
    rig.vcx.simulate_event(gpui::ScrollEvent {
        position: gpui::point(px(800.), px(16.)),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(-60.), px(0.))),
        ..Default::default()
    });
    rig.drain();
    assert!(
        named(&mut rig, "A") < right_at - px(50.),
        "the right bar scrolled"
    );
    assert_eq!(
        named(&mut rig, "Drawer"),
        drawer_at,
        "the drawer's bar did not"
    );
}

#[gpui::test]
fn zoom_fills_the_area_and_always_zooms_out(cx: &mut TestAppContext) {
    let mut rig = rig(cx);
    let (a, b) = (rig.fix("A", true, true), rig.fix("B", false, true));
    let bottom = rig.fix("Bottom", false, false);
    rig.dock(RIGHT, &[&a], 300.);
    rig.dock(DockPlacement::Bottom, &[&bottom], 200.);
    assert_eq!(rig.zoom_from(&bottom), [""; 0], "not zoomable");
    assert!(!rig.zoomed());

    assert_eq!(rig.zoom_from(&a), ["A zoomed true"]);
    assert!(rig.zoomed() && !rig.drawn("content-Center") && !rig.drawn("content-Bottom"));
    let content = rig.vcx.debug_bounds("content-A").expect("A drawn");
    assert_eq!((content.left(), content.size.width), (px(48.), px(1000.)));
    assert_eq!(content.bottom(), px(700.));
    assert_eq!(rig.zoom_from(&a), ["A zoomed false"]);
    assert!(!rig.zoomed() && rig.drawn("content-Center"));

    // A tab that can't zoom shows in the zoomed group: both ways out still work.
    rig.zoom_from(&a);
    rig.add(RIGHT, &b);
    assert!(rig.zoomed(), "adding a tab keeps the zoom");
    assert_eq!(rig.zoom_from(&b), ["B zoomed false"]);
    assert!(!rig.zoomed());
    rig.select(&a);
    rig.zoom_from(&a);
    rig.select(&b);
    let told = rig.act(|area, window, cx| area.set_zoomed_out(window, cx));
    assert_eq!(told, ["B zoomed false"]);
    assert!(!rig.zoomed());

    // The shown tab of a zoomed group leaves: the next one takes over, still zoomed.
    rig.select(&a);
    rig.zoom_from(&a);
    rig.remove(&a);
    assert!(rig.zoomed());
    let content = rig.vcx.debug_bounds("content-B").expect("B drawn");
    assert_eq!(content.size.width, px(1000.));
}

#[gpui::test]
fn locked_areas_last_tabs_and_zoomed_groups_never_close(cx: &mut TestAppContext) {
    let mut rig = rig(cx);
    let (a, b, c) = (
        rig.fix("A", true, true),
        rig.fix("B", false, true),
        rig.fix("C", false, false),
    );
    rig.dock(RIGHT, &[&a, &b, &c], 300.);
    rig.act(|area, window, cx| area.set_locked(true, window, cx));
    rig.select(&b);
    rig.close_from(&b);
    assert_eq!(rig.tabs(RIGHT).0, 3, "a locked area closes nothing");
    rig.act(|area, window, cx| area.set_locked(false, window, cx));
    assert!(rig.close_from(&b).contains(&"B removed".to_owned()));
    rig.select(&c);
    rig.close_from(&c);
    assert_eq!(rig.tabs(RIGHT).0, 2, "C is not closable");

    rig.select(&a);
    rig.zoom_from(&a);
    assert!(rig.zoomed());
    rig.close_from(&a);
    assert_eq!(rig.tabs(RIGHT).0, 2, "a zoomed group closes nothing");
    rig.zoom_from(&a);
    rig.remove(&c);
    rig.close_from(&a);
    assert_eq!(rig.tabs(RIGHT).0, 1, "the last tab of a dock stays");
}
