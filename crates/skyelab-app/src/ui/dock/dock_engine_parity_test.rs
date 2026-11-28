//! Stage 5b acceptance: our dock engine and skin lay a dock out exactly as gpui-base's engine under gpui-component's
//! `DockSkin` did (the app's look before stage 5a). The same fixture panels are drawn by each in a 1000x700 window at
//! the app's 14 px rem, in layouts chosen so every selector names one element, and every selector's bounds must match.

use crate::ui::dock as ours;
use gpui::{
    AppContext as _, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, Styled as _,
    TestAppContext, VisualTestContext, Window, div, px, size,
};
use gpui_base::dock as base;
use gpui_component::dock as theirs;
use std::rc::Rc;

pub(super) struct Fix {
    name: &'static str,
    zoom: bool,
    title_bar: bool,
    focus: FocusHandle,
}

impl EventEmitter<base::PanelEvent> for Fix {}

impl Focusable for Fix {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Fix {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let name = self.name;
        let _render = crate::debug_renders::count(name);
        div()
            .size_full()
            .track_focus(&self.focus)
            .debug_selector(move || format!("content-{name}"))
            .child(name)
    }
}

impl base::Panel for Fix {
    fn panel_name(&self) -> &'static str {
        self.name
    }
    fn closable(&self, _: &gpui::App) -> bool {
        false
    }
}

impl theirs::Panel for Fix {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.name
    }
    fn zoom_control(&self, _: &gpui::App) -> Option<theirs::PanelControl> {
        self.zoom.then_some(theirs::PanelControl::Both)
    }
    fn title_bar(&self, _: &gpui::App) -> bool {
        self.title_bar
    }
}

impl ours::BasePanel for Fix {
    fn panel_name(&self) -> &'static str {
        self.name
    }
    fn closable(&self, _: &gpui::App) -> bool {
        false
    }
}

impl ours::Panel for Fix {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.name
    }
    fn zoom_control(&self, _: &gpui::App) -> Option<ours::PanelControl> {
        self.zoom.then_some(ours::PanelControl::Both)
    }
    fn title_bar(&self, _: &gpui::App) -> bool {
        self.title_bar
    }
}

pub(super) struct Host(gpui::AnyView);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

/// Which docks hold what, at what size, and whether each is open: `(at, panels, size, open)`. Panels are
/// `(name, zoomable)`; a left dock's panel draws no title bar (the sidebar), the others do.
pub(super) type Spec = &'static [(Side, &'static [(&'static str, bool)], f32, bool)];

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Side {
    Left,
    Right,
    Bottom,
}

fn fix(name: &'static str, zoom: bool, title_bar: bool, cx: &mut gpui::App) -> Entity<Fix> {
    cx.new(|cx| Fix {
        name,
        zoom,
        title_bar,
        focus: cx.focus_handle(),
    })
}

/// A drawn dock area: the window, the panels by name, and a way to toggle docks and zoom on either engine.
pub(super) struct Drawn<'a> {
    pub vcx: &'a mut VisualTestContext,
    pub panels: Vec<(&'static str, Entity<Fix>)>,
    pub theirs: Option<Entity<base::DockArea>>,
    pub ours: Option<Entity<ours::DockArea>>,
}

impl Drawn<'_> {
    pub fn panel(&self, name: &str) -> Entity<Fix> {
        let found = self.panels.iter().find(|(n, _)| *n == name);
        found.expect("a fixture panel by that name").1.clone()
    }

    /// Selects `name`'s tab without moving the focus.
    pub fn select(&mut self, name: &str) {
        let id = self.panel(name).entity_id();
        if let Some(a) = self.theirs.clone() {
            a.update_in(self.vcx, |a, w, cx| a.select_panel(id.into(), w, cx));
        }
        if let Some(a) = self.ours.clone() {
            a.update_in(self.vcx, |a, w, cx| a.select_panel(id.into(), w, cx));
        }
        self.vcx.run_until_parked();
    }

    pub fn toggle(&mut self, side: Side) {
        if let Some(a) = self.theirs.clone() {
            a.update_in(self.vcx, |a, w, cx| a.toggle_dock(base_at(side), w, cx));
        }
        if let Some(a) = self.ours.clone() {
            a.update_in(self.vcx, |a, w, cx| a.toggle_dock(our_at(side), w, cx));
        }
        self.vcx.run_until_parked();
    }

    /// Zooms (or un-zooms) the group showing `name`, through the panel's `ToggleZoom` action on either engine.
    pub fn toggle_zoom(&mut self, name: &str) {
        let panel = self.panel(name);
        panel.update_in(self.vcx, |p, w, cx| p.focus.focus(w, cx));
        if self.theirs.is_some() {
            self.vcx.dispatch_action(theirs::ToggleZoom);
        } else {
            self.vcx.dispatch_action(ours::ToggleZoom);
        }
        self.vcx.run_until_parked();
    }

    pub fn bounds(&mut self, which: &[&'static str]) -> Vec<Option<Bounds<Pixels>>> {
        which.iter().map(|s| self.vcx.debug_bounds(s)).collect()
    }
}

fn base_at(side: Side) -> base::DockPlacement {
    match side {
        Side::Left => base::DockPlacement::Left,
        Side::Right => base::DockPlacement::Right,
        Side::Bottom => base::DockPlacement::Bottom,
    }
}

fn our_at(side: Side) -> ours::DockPlacement {
    match side {
        Side::Left => ours::DockPlacement::Left,
        Side::Right => ours::DockPlacement::Right,
        Side::Bottom => ours::DockPlacement::Bottom,
    }
}

/// `spec` drawn by gpui-base's engine under gpui-component's skin (`reference`) or by ours, in a fresh window.
pub(super) fn draw<'a>(reference: bool, spec: Spec, cx: &'a mut TestAppContext) -> Drawn<'a> {
    let mut built: Option<(gpui::AnyView, Option<_>, Option<_>)> = None;
    let (_host, vcx) = cx.add_window_view(|window, cx| {
        // As `WindowRoot` sets it: rem-sized pieces (the toolbar buttons) differ from pixel-sized ones under 16 px.
        window.set_rem_size(crate::ui::ActiveTheme::theme(&**cx).font_size);
        if reference {
            let mut skin = None;
            let area = cx.new(|cx| {
                let s = theirs::DockSkin::new(cx);
                skin = Some(s.clone());
                let renderer: Rc<dyn base::DockAreaRenderer> = s;
                base::DockArea::new("parity", None, window, cx).with_renderer(renderer)
            });
            // As the workspace set it before stage 5a.
            if let Some(s) = skin {
                s.set_toggle_button_visible(false, cx);
            }
            built = Some((area.clone().into(), Some(area), None));
        } else {
            let area = cx.new(|cx| {
                ours::DockArea::new("parity", window, cx).with_renderer(ours::Skin::new(cx))
            });
            built = Some((area.clone().into(), None, Some(area)));
        }
        Host(built.as_ref().expect("just built").0.clone())
    });
    let (_, theirs_area, ours_area) = built.expect("the window view was built");
    vcx.simulate_resize(size(px(1000.), px(700.)));
    let mut panels = Vec::new();
    let center = vcx.update(|_, cx| fix("Center", false, false, cx));
    panels.push(("Center", center.clone()));
    let mut docks = Vec::new();
    for (side, names, px_size, open) in spec {
        let ps: Vec<_> = names
            .iter()
            .map(|(n, zoom)| {
                let p = vcx.update(|_, cx| fix(n, *zoom, *side != Side::Left, cx));
                panels.push((*n, p.clone()));
                p
            })
            .collect();
        docks.push((*side, ps, px(*px_size), *open));
    }
    if let Some(area) = theirs_area.clone() {
        area.update_in(vcx, |area, window, cx| {
            let tabs = |ps: &[Entity<Fix>], cx: &gpui::App| {
                ps.iter().fold(base::DockLayout::tabs(), |l, p| {
                    l.panel_view(theirs::panel_handle(p.clone()), cx)
                })
            };
            area.set_center(tabs(std::slice::from_ref(&center), cx), window, cx);
            for (side, ps, s, open) in &docks {
                area.set_dock(base_at(*side), tabs(ps, cx), window, cx);
                area.set_dock_size(base_at(*side), *s, window, cx);
                if area.is_dock_open(base_at(*side)) != *open {
                    area.toggle_dock(base_at(*side), window, cx);
                }
            }
            area.set_locked(true, window, cx);
        });
    }
    if let Some(area) = ours_area.clone() {
        area.update_in(vcx, |area, window, cx| {
            let tabs = |ps: &[Entity<Fix>], cx: &gpui::App| {
                ps.iter().fold(ours::DockLayout::tabs(), |l, p| {
                    l.panel_view(ours::panel_handle(p.clone()), cx)
                })
            };
            area.set_center(tabs(std::slice::from_ref(&center), cx), window, cx);
            for (side, ps, s, open) in &docks {
                area.set_dock(our_at(*side), tabs(ps, cx), window, cx);
                area.set_dock_size(our_at(*side), *s, window, cx);
                if area.is_dock_open(our_at(*side)) != *open {
                    area.toggle_dock(our_at(*side), window, cx);
                }
            }
            area.set_locked(true, window, cx);
        });
    }
    vcx.run_until_parked();
    Drawn {
        vcx,
        panels,
        theirs: theirs_area,
        ours: ours_area,
    }
}

/// What to do to a drawn layout before measuring it.
#[derive(Clone, Copy)]
enum Then {
    Nothing,
    Toggle(Side),
    Zoom(&'static str),
}

const TWO_TABS: Spec = &[(Side::Right, &[("A", true), ("B", false)], 300., true)];
const TITLE: Spec = &[(Side::Right, &[("A", true)], 300., true)];
const BOTTOM: Spec = &[(Side::Bottom, &[("Bottom", false)], 300., true)];
const ALL: Spec = &[
    (Side::Left, &[("Left", false)], 256., true),
    (Side::Right, &[("A", true)], 400., true),
    (Side::Bottom, &[("Bottom", false)], 280., true),
];
const SIDES_CLOSED: Spec = &[
    (Side::Left, &[("Left", false)], 256., false),
    (Side::Right, &[("A", true)], 400., false),
];

const ZOOM: &str = "dock-tab-bar-zoom-control";

#[gpui::test]
fn our_engine_lays_out_like_gpui_base_s(cx: &mut TestAppContext) {
    super::super::button_test::setup(cx);
    let cases: [(&str, Spec, Then, &[&'static str]); 7] = [
        (
            "two tabs",
            TWO_TABS,
            Then::Nothing,
            &[
                "dock-tab-0",
                "dock-tab-1",
                ZOOM,
                "content-Center",
                "content-A",
            ],
        ),
        (
            "title bar",
            TITLE,
            Then::Nothing,
            &[ZOOM, "content-Center", "content-A"],
        ),
        (
            "bottom open",
            BOTTOM,
            Then::Nothing,
            &["content-Center", "content-Bottom"],
        ),
        // The center's height measures the strip.
        (
            "bottom strip",
            BOTTOM,
            Then::Toggle(Side::Bottom),
            &["content-Center"],
        ),
        (
            "all open",
            ALL,
            Then::Nothing,
            &[
                "content-Left",
                "content-Center",
                "content-A",
                "content-Bottom",
                ZOOM,
            ],
        ),
        (
            "sides closed",
            SIDES_CLOSED,
            Then::Nothing,
            &["content-Center"],
        ),
        ("zoomed", ALL, Then::Zoom("A"), &["content-A", ZOOM]),
    ];
    for (name, spec, then, selectors) in cases {
        let mut measured = Vec::new();
        for reference in [true, false] {
            let mut d = draw(reference, spec, cx);
            match then {
                Then::Nothing => {}
                Then::Toggle(side) => d.toggle(side),
                Then::Zoom(panel) => d.toggle_zoom(panel),
            }
            measured.push(d.bounds(selectors));
        }
        for (i, s) in selectors.iter().enumerate() {
            assert!(measured[0][i].is_some(), "{name}: gpui-base draws {s}");
            assert_eq!(measured[1][i], measured[0][i], "{name}: {s}");
        }
    }
}
