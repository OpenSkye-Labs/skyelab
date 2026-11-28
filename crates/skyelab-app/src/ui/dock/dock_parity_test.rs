//! Stage 5a acceptance: our dock skin lays a dock out exactly as gpui-component's `DockSkin` does. The same panels are
//! drawn under each skin in a window of the same size, in three layouts so every selector names one element: a center
//! without a title bar beside a right dock with two tabs (one zoomable), the same with one panel (a title bar), and a center over a bottom dock, open and
//! then closed to its strip (the center's height measures it). Every tab, the zoom button and each panel's content region must have the same bounds.

use crate::ui::dock as ours;
use gpui::{
    AppContext as _, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, Styled as _,
    TestAppContext, VisualTestContext, Window, div, px, size,
};
use gpui_base::dock::{
    DockArea, DockAreaRenderer, DockLayout, DockPlacement, PanelEvent, PanelView,
};
use gpui_component::dock as theirs;
use std::{rc::Rc, sync::Arc};

struct Fix {
    name: &'static str,
    zoom: bool,
    title_bar: bool,
    focus: FocusHandle,
}

impl EventEmitter<PanelEvent> for Fix {}

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
            .debug_selector(move || format!("content-{name}"))
            .child(name)
    }
}

impl gpui_base::dock::Panel for Fix {
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

#[derive(Clone, Copy, PartialEq)]
enum Skin {
    Theirs,
    Ours,
}

struct Host(Entity<DockArea>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Layout {
    Right,
    /// The right dock with one zoomable panel: a title bar, as the app's "Open a surface" tab draws.
    Title,
    Bottom,
}

const RIGHT: [&str; 5] = [
    "dock-tab-0",
    "dock-tab-1",
    "dock-tab-bar-zoom-control",
    "content-Center",
    "content-A",
];
const TITLE: [&str; 3] = ["dock-tab-bar-zoom-control", "content-Center", "content-A"];
// One panel alone gets a title bar, not tabs: its strip shows in the center's height.
const BOTTOM: [&str; 2] = ["content-Center", "content-Bottom"];

fn fix(name: &'static str, zoom: bool, title_bar: bool, cx: &mut gpui::App) -> Entity<Fix> {
    cx.new(|cx| Fix {
        name,
        zoom,
        title_bar,
        focus: cx.focus_handle(),
    })
}

fn handle(skin: Skin, panel: Entity<Fix>) -> Arc<dyn PanelView> {
    match skin {
        Skin::Theirs => theirs::panel_handle(panel),
        Skin::Ours => ours::panel_handle(panel),
    }
}

fn bounds(vcx: &mut VisualTestContext, which: &[&'static str]) -> Vec<Option<Bounds<Pixels>>> {
    which.iter().map(|s| vcx.debug_bounds(s)).collect()
}

type Drawn = Vec<Option<Bounds<Pixels>>>;

/// `layout` drawn under `skin`: the bounds of its selectors, then (bottom layout only) again with the dock closed.
fn draw(skin: Skin, layout: Layout, cx: &mut TestAppContext) -> (Drawn, Drawn) {
    let mut built = None;
    let (_host, vcx) = cx.add_window_view(|window, cx| {
        let mut their_skin = None;
        let area = cx.new(|cx| {
            let renderer: Rc<dyn DockAreaRenderer> = match skin {
                Skin::Theirs => {
                    let s = theirs::DockSkin::new(cx);
                    their_skin = Some(s.clone());
                    s
                }
                Skin::Ours => ours::Skin::new(cx),
            };
            DockArea::new("parity", None, window, cx).with_renderer(renderer)
        });
        // As the workspace sets it, once the area exists (workspace/mod.rs before stage 5a).
        if let Some(s) = their_skin {
            s.set_toggle_button_visible(false, cx);
        }
        built = Some(area.clone());
        Host(area)
    });
    let area = built.expect("the window view was built");
    vcx.simulate_resize(size(px(1000.), px(700.)));
    area.update_in(vcx, |area, window, cx| {
        let tabs = |ps: Vec<Entity<Fix>>, cx: &gpui::App| {
            ps.into_iter()
                .fold(DockLayout::tabs(), |l, p| l.panel_view(handle(skin, p), cx))
        };
        let center = fix("Center", false, false, cx);
        area.set_center(tabs(vec![center], cx), window, cx);
        let at = if layout == Layout::Right {
            let a = fix("A", true, true, cx);
            let b = fix("B", false, true, cx);
            area.set_dock(DockPlacement::Right, tabs(vec![a, b], cx), window, cx);
            DockPlacement::Right
        } else if layout == Layout::Title {
            let a = fix("A", true, true, cx);
            area.set_dock(DockPlacement::Right, tabs(vec![a], cx), window, cx);
            DockPlacement::Right
        } else {
            let bottom = fix("Bottom", false, true, cx);
            area.set_dock(DockPlacement::Bottom, tabs(vec![bottom], cx), window, cx);
            DockPlacement::Bottom
        };
        area.set_dock_size(at, px(300.), window, cx);
        if !area.is_dock_open(at) {
            area.toggle_dock(at, window, cx);
        }
        area.set_locked(true, window, cx);
    });
    vcx.run_until_parked();
    let selectors: &[&'static str] = match layout {
        Layout::Right => &RIGHT,
        Layout::Title => &TITLE,
        Layout::Bottom => &BOTTOM,
    };
    let open = bounds(vcx, selectors);
    if layout != Layout::Bottom {
        return (open, Vec::new());
    }
    area.update_in(vcx, |area, window, cx| {
        area.toggle_dock(DockPlacement::Bottom, window, cx)
    });
    vcx.run_until_parked();
    (open, bounds(vcx, selectors))
}

#[gpui::test]
fn our_dock_skin_lays_out_like_gpui_component_s(cx: &mut TestAppContext) {
    super::super::button_test::setup(cx);
    for (layout, selectors) in [
        (Layout::Right, &RIGHT[..]),
        (Layout::Title, &TITLE[..]),
        (Layout::Bottom, &BOTTOM[..]),
    ] {
        let (their_open, their_closed) = draw(Skin::Theirs, layout, cx);
        let (our_open, our_closed) = draw(Skin::Ours, layout, cx);
        for (i, s) in selectors.iter().enumerate() {
            assert!(
                their_open[i].is_some(),
                "theirs draws {s} with the dock open"
            );
            assert_eq!(our_open[i], their_open[i], "{s}, dock open");
        }
        assert_eq!(
            our_closed, their_closed,
            "the closed bottom dock and its strip"
        );
    }
}
