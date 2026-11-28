//! Stage 5b acceptance: our dock engine's sizes. Resize drags keep their clamps and are measured from the area (not
//! the window), the bottom dock follows the pointer under its minimum and snaps on release, and `LayoutChanged` (what
//! saves the layout) comes when it did. Every expected value was measured on gpui-base's engine at planning.

use super::dock_engine_test::{RIGHT, Rig, rig};
use crate::ui::dock::DockPlacement;
use gpui::{Modifiers, MouseButton, Pixels, Point, TestAppContext, point, px};

const LEFT: DockPlacement = DockPlacement::Left;
const BOTTOM: DockPlacement = DockPlacement::Bottom;

fn at(x: f32, y: f32) -> Point<Pixels> {
    point(px(x), px(y))
}

fn press(rig: &mut Rig<'_>, p: Point<Pixels>) {
    rig.vcx
        .simulate_mouse_down(p, MouseButton::Left, Modifiers::none());
}

fn move_to(rig: &mut Rig<'_>, p: Point<Pixels>) -> Vec<String> {
    rig.vcx
        .simulate_mouse_move(p, MouseButton::Left, Modifiers::none());
    rig.drain()
}

fn release(rig: &mut Rig<'_>, p: Point<Pixels>) -> Vec<String> {
    rig.vcx
        .simulate_mouse_up(p, MouseButton::Left, Modifiers::none());
    rig.drain()
}

/// A press, a first small move (it starts GPUI's drag), one move to `to`, and the release there.
fn drag(rig: &mut Rig<'_>, from: Point<Pixels>, to: Point<Pixels>) {
    press(rig, from);
    move_to(rig, point(from.x + px(3.), from.y + px(3.)));
    move_to(rig, to);
    release(rig, to);
}

fn size(rig: &mut Rig<'_>, at: DockPlacement) -> Pixels {
    let size = rig.area.read_with(rig.vcx, |a, _| a.dock_size(at));
    size.expect("a dock there")
}

fn open(rig: &mut Rig<'_>, at: DockPlacement) -> bool {
    rig.area.read_with(rig.vcx, |a, _| a.is_dock_open(at))
}

fn center_bottom(rig: &mut Rig<'_>) -> Pixels {
    let center = rig.vcx.debug_bounds("content-Center");
    center.expect("the center is drawn").bottom()
}

/// The area is 1000x700 at (48, 0): left dock 200 (48..248), right 400 (648..1048), bottom 200 (500..700).
fn three_docks(cx: &mut TestAppContext) -> Rig<'_> {
    let mut rig = rig(cx);
    let [l, r, b] = ["L", "R", "B"].map(|n| rig.fix(n, false, false));
    rig.dock(LEFT, &[&l], 200.);
    rig.dock(RIGHT, &[&r], 400.);
    rig.dock(BOTTOM, &[&b], 200.);
    rig.events();
    rig
}

#[gpui::test]
fn side_docks_resize_within_the_area(cx: &mut TestAppContext) {
    let mut rig = three_docks(cx);
    drag(&mut rig, at(649., 100.), at(548., 100.));
    assert_eq!(
        size(&mut rig, RIGHT),
        px(500.),
        "measured from the area's right edge"
    );
    assert_eq!(rig.events(), 1, "one LayoutChanged, at release");
    drag(&mut rig, at(549., 100.), at(0., 100.));
    assert_eq!(
        size(&mut rig, RIGHT),
        px(700.),
        "the left dock and 100 px of center stay"
    );
    rig.act(|a, window, cx| a.set_dock_size(RIGHT, px(400.), window, cx));
    drag(&mut rig, at(247., 100.), at(348., 100.));
    assert_eq!(
        size(&mut rig, LEFT),
        px(300.),
        "measured from the area's left edge"
    );
    drag(&mut rig, at(347., 100.), at(60., 100.));
    assert_eq!(size(&mut rig, LEFT), px(100.), "never under 100 px");
    // A closed dock opposite still counts in the clamp.
    rig.act(|a, window, cx| a.set_dock_size(LEFT, px(200.), window, cx));
    rig.toggle(LEFT);
    drag(&mut rig, at(649., 100.), at(0., 100.));
    assert_eq!(size(&mut rig, RIGHT), px(700.));
}

#[gpui::test]
fn the_bottom_dock_follows_the_pointer_and_snaps(cx: &mut TestAppContext) {
    let mut rig = three_docks(cx);
    assert_eq!(center_bottom(&mut rig), px(500.));
    // One move to 10 px over the area's bottom, then the release: closed to the strip, its size kept.
    drag(&mut rig, at(400., 501.), at(400., 690.));
    assert!(!open(&mut rig, BOTTOM));
    assert_eq!(
        center_bottom(&mut rig),
        px(700. - 29.),
        "the 29 px strip stays"
    );
    assert!(!rig.drawn("content-B"));
    assert_eq!(rig.toggle(BOTTOM), ["B active true"]);
    assert_eq!(
        size(&mut rig, BOTTOM),
        px(200.),
        "it reopens at its last size"
    );

    // Under the minimum it follows the pointer; past the strip its panel is told it is hidden, mid-drag.
    press(&mut rig, at(400., 501.));
    move_to(&mut rig, at(403., 504.));
    assert_eq!(move_to(&mut rig, at(400., 640.)), [""; 0]);
    assert_eq!(center_bottom(&mut rig), px(640.), "60 px, drawn live");
    assert_eq!(move_to(&mut rig, at(400., 680.)), ["B active false"]);
    assert_eq!(center_bottom(&mut rig), px(671.), "never under the strip");
    assert_eq!(
        move_to(&mut rig, at(400., 630.)),
        ["B active true"],
        "and back"
    );
    release(&mut rig, at(400., 630.));
    assert!(open(&mut rig, BOTTOM));
    assert_eq!(
        size(&mut rig, BOTTOM),
        px(100.),
        "nearer the minimum: it opens there"
    );
    assert_eq!(center_bottom(&mut rig), px(600.));

    // Nearer the strip than the minimum (60 < 64.5) it closes.
    drag(&mut rig, at(400., 601.), at(400., 640.));
    assert!(!open(&mut rig, BOTTOM));
    // The closed strip's edge drags it open again.
    drag(&mut rig, at(400., 672.), at(400., 450.));
    assert!(open(&mut rig, BOTTOM));
    assert_eq!(size(&mut rig, BOTTOM), px(250.));
    drag(&mut rig, at(400., 451.), at(400., 0.));
    assert_eq!(size(&mut rig, BOTTOM), px(600.), "100 px of center stay");
}

#[gpui::test]
fn layout_changed_comes_for_changes_only(cx: &mut TestAppContext) {
    let mut rig = three_docks(cx);
    let (x, y) = (rig.fix("X", false, true), rig.fix("Y", false, true));
    let mut seen = Vec::new();
    rig.add(RIGHT, &x);
    seen.push(("add", rig.events()));
    rig.add(RIGHT, &y);
    rig.events();
    rig.select(&y);
    seen.push(("select the shown tab", rig.events()));
    rig.select(&x);
    seen.push(("select another", rig.events()));
    rig.remove(&y);
    seen.push(("remove", rig.events()));
    rig.remove(&y);
    seen.push(("remove again", rig.events()));
    rig.act(|a, window, cx| a.set_dock_size(RIGHT, px(400.), window, cx));
    seen.push(("the same size", rig.events()));
    rig.act(|a, window, cx| a.set_dock_size(RIGHT, px(420.), window, cx));
    seen.push(("another size", rig.events()));
    rig.act(|a, window, cx| a.set_dock_size(RIGHT, px(20.), window, cx));
    seen.push(("under the minimum", rig.events()));
    assert_eq!(size(&mut rig, RIGHT), px(100.));
    rig.toggle(RIGHT);
    seen.push(("toggle", rig.events()));
    rig.act(|a, window, cx| a.set_locked(true, window, cx));
    seen.push(("lock", rig.events()));
    assert_eq!(
        seen,
        [
            ("add", 1),
            ("select the shown tab", 0),
            ("select another", 1),
            ("remove", 1),
            ("remove again", 0),
            ("the same size", 0),
            ("another size", 1),
            ("under the minimum", 1),
            ("toggle", 1),
            ("lock", 0),
        ]
    );
}
