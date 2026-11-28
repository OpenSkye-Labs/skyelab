use super::{A11yScrollArea as _, Area, Seen, area_for, area_recorder, scroll_for, wheel};
use crate::ui::Button;
use crate::ui::v_flex;
use gpui::{
    Bounds, Context, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render,
    StatefulInteractiveElement as _, Styled as _, TestAppContext, VisualTestContext, Window, div,
    point, px, size,
};
use gpui_base::test_support::snapshots;

/// A 200 px box at the window's origin: `Near` at its top, `Far` 600 px further down.
struct Long(Area);

impl Render for Long {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("scroller")
            .size(px(200.))
            .overflow_y_scroll()
            .child(area_recorder(self.0.clone(), |_| {}))
            .child(
                v_flex()
                    .child(Button::new("near").label("Near").on_click(|_, _, _| {}))
                    .child(div().h(px(600.)))
                    .child(Button::new("far").label("Far").on_click(|_, _, _| {})),
            )
    }
}

fn the_box() -> Bounds<Pixels> {
    Bounds::new(point(px(0.), px(0.)), size(px(200.), px(200.)))
}

fn long(cx: &mut TestAppContext) -> (Area, &mut VisualTestContext) {
    crate::ui::button_test::setup(cx);
    let area: Area = Default::default();
    let a = area.clone();
    let (_, cx) = cx.add_window_view(move |_, _| Long(a));
    cx.run_until_parked();
    (area, cx)
}

fn bounds_of(cx: &mut VisualTestContext, label: &str) -> Bounds<Pixels> {
    cx.update(|window, _| snapshots(window))
        .into_iter()
        .find(|s| s.label() == Some(label))
        .expect("the button is in the tree")
        .bounds()
}

/// What GPUI hands a control drawn in `area`: the clips intersected, its own included.
fn seen(bounds: Bounds<Pixels>, area: Bounds<Pixels>) -> Seen {
    Seen {
        bounds,
        clip: bounds.intersect(&area),
    }
}

fn inside(b: Bounds<Pixels>, clip: Bounds<Pixels>) -> bool {
    b.top() >= clip.top() && b.bottom() <= clip.bottom()
}

#[gpui::test]
fn an_offscreen_button_scrolls_into_the_recorded_area(cx: &mut TestAppContext) {
    let (area, cx) = long(cx);
    let shown = area.get().expect("the box recorded where it is");
    assert_eq!(shown, the_box());
    let far = bounds_of(cx, "Far");
    assert!(far.top() >= shown.bottom(), "starts below the box: {far:?}");

    cx.simulate_event(scroll_for(seen(far, shown), &[shown]).expect("a scroll is needed"));
    cx.run_until_parked();

    let after = bounds_of(cx, "Far");
    assert!(
        inside(after, shown),
        "inside the box after one wheel event: {after:?}"
    );
    assert!(
        scroll_for(seen(after, shown), &[shown]).is_none(),
        "nothing left to scroll"
    );
}

#[gpui::test]
fn a_box_scrolled_past_its_own_height_still_records_where_it_is(cx: &mut TestAppContext) {
    let (area, cx) = long(cx);
    cx.simulate_event(wheel(point(px(100.), px(100.)), px(-1000.)));
    cx.run_until_parked();
    let near = bounds_of(cx, "Near");
    assert!(
        near.bottom() <= px(0.),
        "Near scrolled fully above the box: {near:?}"
    );
    assert_eq!(area.get(), Some(the_box()));

    cx.simulate_event(scroll_for(seen(near, the_box()), &[the_box()]).expect("scroll back"));
    cx.run_until_parked();
    assert!(
        inside(bounds_of(cx, "Near"), the_box()),
        "Near is back in view"
    );
}

#[gpui::test]
fn a_button_cut_off_at_the_bottom_scrolls_fully_in(cx: &mut TestAppContext) {
    let (_, cx) = long(cx);
    // Leave `Far` half under the box's bottom edge.
    let far = bounds_of(cx, "Far");
    cx.simulate_event(wheel(
        point(px(100.), px(100.)),
        the_box().bottom() - far.center().y,
    ));
    cx.run_until_parked();
    let cut = bounds_of(cx, "Far");
    assert!(
        cut.top() < the_box().bottom() && cut.bottom() > the_box().bottom(),
        "{cut:?}"
    );

    cx.simulate_event(scroll_for(seen(cut, the_box()), &[the_box()]).expect("a scroll is needed"));
    cx.run_until_parked();
    assert!(inside(bounds_of(cx, "Far"), the_box()));
}

struct Padded(gpui::ScrollHandle);

impl Render for Padded {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("padded")
            .size(px(200.))
            .p_4()
            .overflow_y_scroll()
            .track_scroll(&self.0)
            .a11y_scroll_area()
            .child(div().h(px(50.)))
    }
}

#[gpui::test]
fn the_area_recorder_adds_no_scroll_range(cx: &mut TestAppContext) {
    crate::ui::button_test::setup(cx);
    let handle = gpui::ScrollHandle::new();
    let h = handle.clone();
    let (_, cx) = cx.add_window_view(move |_, _| Padded(h));
    cx.run_until_parked();
    assert_eq!(
        handle.max_offset().y,
        px(0.),
        "short content in a padded box does not scroll"
    );
}

#[test]
fn a_control_only_clipped_sideways_never_scrolls() {
    let bounds = Bounds::new(point(px(150.), px(10.)), size(px(100.), px(30.)));
    assert!(scroll_for(seen(bounds, the_box()), &[the_box()]).is_none());
}

#[test]
fn the_area_is_the_frontmost_one_spanning_the_controls_columns() {
    let page = Bounds::new(point(px(0.), px(0.)), size(px(800.), px(600.)));
    let dialog = Bounds::new(point(px(100.), px(50.)), size(px(500.), px(400.)));
    // A popover over part of the dialog's column, drawn last.
    let popover = Bounds::new(point(px(150.), px(300.)), size(px(200.), px(100.)));
    let control = Bounds::new(point(px(120.), px(700.)), size(px(460.), px(24.)));
    let target = seen(control, dialog);
    assert_eq!(area_for(target, &[page, dialog, popover]), Some(dialog));
    assert_eq!(area_for(seen(control, page), &[page]), Some(page));
}
