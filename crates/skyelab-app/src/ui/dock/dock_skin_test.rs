//! Stage 5a acceptance: our dock skin (`ui::dock::Skin`) on gpui-base's engine. Tabs select and focus, the zoom
//! button, the `ToggleZoom` action and the "…" menu's Zoom item zoom, the menu's Close removes one tab, the resize
//! handles resize and the bottom one closes its dock, and the tab bar reads right in the a11y tree. Each dock area has
//! one "…" button unless a test adds the bottom dock, so selectors name one element.

use crate::ui::dock::{
    self as dock, DockArea, DockLayout, DockPlacement, PanelControl, PanelEvent, ToggleZoom,
};
use crate::ui::{PopupMenu, PopupMenuItem};
use gpui::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, Modifiers, MouseButton, ParentElement as _, Render, Role, Styled as _,
    TestAppContext, VisualTestContext, Window, div, point, px, size,
};
use gpui_base::test_support::snapshots;
use std::{cell::Cell, rc::Rc};

struct Fix {
    name: &'static str,
    zoom: bool,
    closable: bool,
    clicked: Rc<Cell<bool>>,
    focus: FocusHandle,
}

impl Fix {
    fn new(name: &'static str, zoom: bool, closable: bool, cx: &mut gpui::App) -> Entity<Self> {
        cx.new(|cx| Self {
            name,
            zoom,
            closable,
            clicked: Rc::default(),
            focus: cx.focus_handle(),
        })
    }
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
            .track_focus(&self.focus)
            .debug_selector(move || format!("content-{name}"))
            .child(name)
    }
}

impl gpui_base::dock::Panel for Fix {
    fn panel_name(&self) -> &'static str {
        self.name
    }
    fn closable(&self, _: &gpui::App) -> bool {
        self.closable
    }
}

impl dock::Panel for Fix {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.name
    }
    fn zoom_control(&self, _: &gpui::App) -> Option<PanelControl> {
        self.zoom.then_some(PanelControl::Both)
    }
    fn dropdown_menu(&mut self, menu: PopupMenu, _: &mut Window, _: &mut Context<Self>) -> PopupMenu {
        let clicked = self.clicked.clone();
        menu.item(PopupMenuItem::new("Fixture item").on_click(move |_, _, _| clicked.set(true)))
    }
}

struct Panels {
    center: Entity<Fix>,
    a: Entity<Fix>,
    b: Entity<Fix>,
    bottom: Entity<Fix>,
}

struct Host(Entity<DockArea>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

/// A 1000x700 window: a center and a right dock with tabs A (zoomable) and B (closable); with `bottom`, an open bottom
/// dock too. `locked` as the workspace sets it: gpui-base then lets no group close a tab.
fn area(
    cx: &mut TestAppContext,
    locked: bool,
    bottom: bool,
) -> (Entity<DockArea>, Panels, &mut VisualTestContext) {
    super::super::button_test::setup(cx);
    let panels = cx.update(|cx| Panels {
        center: Fix::new("Center", false, false, cx),
        a: Fix::new("A", true, false, cx),
        b: Fix::new("B", false, true, cx),
        bottom: Fix::new("Bottom", false, false, cx),
    });
    let mut built = None;
    let (_host, vcx) = cx.add_window_view(|window, cx| {
        let area = cx.new(|cx| {
            DockArea::new("skin-test", None, window, cx).with_renderer(dock::Skin::new(cx))
        });
        built = Some(area.clone());
        Host(area)
    });
    let area = built.expect("the window view was built");
    vcx.simulate_resize(size(px(1000.), px(700.)));
    let p = &panels;
    area.update_in(vcx, |area, window, cx| {
        let tabs = |ps: &[&Entity<Fix>], cx: &gpui::App| {
            ps.iter().fold(DockLayout::tabs(), |l, p| {
                l.panel_view(dock::panel_handle((*p).clone()), cx)
            })
        };
        area.set_center(tabs(&[&p.center], cx), window, cx);
        area.set_dock(DockPlacement::Right, tabs(&[&p.a, &p.b], cx), window, cx);
        area.set_dock_size(DockPlacement::Right, px(400.), window, cx);
        // A new dock starts open in gpui-base; toggling it would close it.
        if !area.is_dock_open(DockPlacement::Right) {
            area.toggle_dock(DockPlacement::Right, window, cx);
        }
        if bottom {
            area.set_dock(DockPlacement::Bottom, tabs(&[&p.bottom], cx), window, cx);
            area.set_dock_size(DockPlacement::Bottom, px(200.), window, cx);
            if !area.is_dock_open(DockPlacement::Bottom) {
                area.toggle_dock(DockPlacement::Bottom, window, cx);
            }
        }
        area.set_locked(locked, window, cx);
    });
    vcx.run_until_parked();
    (area, panels, vcx)
}

fn click_at(vcx: &mut VisualTestContext, at: gpui::Point<gpui::Pixels>) {
    vcx.simulate_click(at, Modifiers::none());
    vcx.run_until_parked();
}

fn click(vcx: &mut VisualTestContext, selector: &'static str) {
    let at = vcx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} drawn"))
        .center();
    click_at(vcx, at);
}

fn click_label(vcx: &mut VisualTestContext, label: &str) {
    let at = vcx
        .update(|window, _| snapshots(window))
        .into_iter()
        .find(|s| s.label() == Some(label) && s.visible())
        .unwrap_or_else(|| panic!("{label} drawn"))
        .bounds()
        .center();
    click_at(vcx, at);
}

fn zoomed(area: &Entity<DockArea>, vcx: &mut VisualTestContext) -> bool {
    area.read_with(vcx, |a, _| a.is_zoomed())
}

#[gpui::test]
fn a_clicked_tab_is_shown_and_takes_the_focus(cx: &mut TestAppContext) {
    let (_area, p, vcx) = area(cx, true, false);
    assert!(vcx.debug_bounds("content-A").is_some(), "A shows first");
    click(vcx, "dock-tab-1");
    assert!(vcx.debug_bounds("content-B").is_some(), "B shows after its tab is clicked");
    assert!(vcx.debug_bounds("content-A").is_none(), "and A no longer does");
    let focused = p.b.update_in(vcx, |b, window, _| b.focus.is_focused(window));
    assert!(focused, "the click moved focus into B");
    // Clicking the tab already shown still focuses it (gpui-base only moves focus when the tab changes).
    p.center.update_in(vcx, |c, window, cx| c.focus.focus(window, cx));
    click(vcx, "dock-tab-1");
    let focused = p.b.update_in(vcx, |b, window, _| b.focus.is_focused(window));
    assert!(focused, "a click on the shown tab focuses it too");
}

#[gpui::test]
fn the_zoom_button_and_the_action_zoom_in_and_out(cx: &mut TestAppContext) {
    let (area, p, vcx) = area(cx, true, false);
    click(vcx, "dock-tab-bar-zoom-control");
    assert!(zoomed(&area, vcx), "the zoom button zooms in");
    click(vcx, "dock-tab-bar-zoom-control");
    assert!(!zoomed(&area, vcx), "and out");
    p.a.update_in(vcx, |a, window, cx| a.focus.focus(window, cx));
    vcx.dispatch_action(ToggleZoom);
    vcx.run_until_parked();
    assert!(zoomed(&area, vcx), "ToggleZoom from the panel zooms in");
    vcx.dispatch_action(ToggleZoom);
    vcx.run_until_parked();
    assert!(!zoomed(&area, vcx), "and out");
}

fn menu_items(vcx: &mut VisualTestContext) -> Vec<String> {
    let snaps = vcx.update(|window, _| snapshots(window));
    let menu = snaps
        .iter()
        .find(|s| s.role() == Some(Role::Menu) && s.visible())
        .expect("our menu (role Menu) is open")
        .bounds();
    snaps
        .iter()
        .filter(|s| s.role() == Some(Role::MenuItem) && s.visible())
        .filter(|s| menu.contains(&s.bounds().center()))
        .filter_map(|s| s.label().map(str::to_owned))
        .collect()
}

#[gpui::test]
fn the_menu_holds_the_panel_s_items_then_zoom(cx: &mut TestAppContext) {
    let (area, p, vcx) = area(cx, true, false);
    click(vcx, "dock-tab-bar-menu");
    assert_eq!(menu_items(vcx), ["Fixture item", "Zoom In"], "A is not closable: no Close");
    click_label(vcx, "Fixture item");
    assert!(p.a.read_with(vcx, |a, _| a.clicked.get()), "the panel's item ran");
    click(vcx, "dock-tab-bar-menu");
    click_label(vcx, "Zoom In");
    assert!(zoomed(&area, vcx), "the menu's Zoom In zooms");
}

#[gpui::test]
fn a_locked_dock_offers_no_close(cx: &mut TestAppContext) {
    // gpui-base refuses every close in a locked area (`TabGroupContext::is_closable`), so neither skin offers one.
    let (_area, _p, vcx) = area(cx, true, false);
    click(vcx, "dock-tab-1");
    click(vcx, "dock-tab-bar-menu");
    assert_eq!(menu_items(vcx), ["Fixture item", "Zoom In"], "B is closable, the dock is locked");
}

#[gpui::test]
fn close_in_the_menu_removes_only_that_tab(cx: &mut TestAppContext) {
    let (area, p, vcx) = area(cx, false, false);
    click(vcx, "dock-tab-bar-menu");
    assert_eq!(menu_items(vcx), ["Fixture item", "Zoom In"], "A is not closable");
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    click(vcx, "dock-tab-1");
    click(vcx, "dock-tab-bar-menu");
    assert_eq!(menu_items(vcx), ["Fixture item", "Zoom In", "Close"]);
    click_label(vcx, "Close");
    let (has_a, has_b, open) = area.read_with(vcx, |area, _| {
        (
            area.panel(p.a.entity_id().into()).is_some(),
            area.panel(p.b.entity_id().into()).is_some(),
            area.is_dock_open(DockPlacement::Right),
        )
    });
    assert!(!has_b && has_a && open, "B closed, A and the dock stay");
}

#[gpui::test]
fn the_handles_resize_and_the_bottom_one_closes_its_dock(cx: &mut TestAppContext) {
    let (area, _p, vcx) = area(cx, true, true);
    let right = |vcx: &mut VisualTestContext| {
        area.read_with(vcx, |a, _| a.dock_size(DockPlacement::Right))
    };
    assert_eq!(right(vcx), Some(px(400.)));
    // The right dock's left edge sits at 600; its handle straddles it.
    let y = px(100.);
    vcx.simulate_mouse_down(point(px(600.), y), MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_move(point(px(550.), y), MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_move(point(px(500.), y), MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_up(point(px(500.), y), MouseButton::Left, Modifiers::none());
    vcx.run_until_parked();
    assert_eq!(right(vcx), Some(px(500.)), "100 px toward the center makes it 100 px wider");

    // The bottom dock's top edge sits at 500 (700 - 200); dragged to the window's bottom, it closes.
    let x = px(200.);
    vcx.simulate_mouse_down(point(x, px(500.)), MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_move(point(x, px(600.)), MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_move(point(x, px(699.)), MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_up(point(x, px(699.)), MouseButton::Left, Modifiers::none());
    vcx.run_until_parked();
    let open = area.read_with(vcx, |a, _| a.is_dock_open(DockPlacement::Bottom));
    assert!(!open, "released below its strip, the bottom dock is closed");
}

#[gpui::test]
fn the_tab_bar_reads_right_in_the_a11y_tree(cx: &mut TestAppContext) {
    let (_area, _p, vcx) = area(cx, true, false);
    let snaps = vcx.update(|window, _| snapshots(window));
    let tab = |name: &str| {
        snaps
            .iter()
            .find(|s| s.role() == Some(Role::Tab) && s.label() == Some(name))
            .unwrap_or_else(|| panic!("a Tab named {name}"))
    };
    assert_eq!(tab("A").selected(), Some(true), "A is the shown tab");
    assert_ne!(tab("B").selected(), Some(true));
    for selector in ["dock-tab-bar-zoom-control", "dock-tab-bar-menu"] {
        let at = vcx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} drawn"))
            .center();
        let button = snaps
            .iter()
            .find(|s| s.role() == Some(Role::Button) && s.visible() && s.bounds().contains(&at))
            .unwrap_or_else(|| panic!("{selector} is a Button in the a11y tree"));
        assert!(
            button.label().is_some_and(|l| !l.is_empty()),
            "{selector} has a name"
        );
    }
}
