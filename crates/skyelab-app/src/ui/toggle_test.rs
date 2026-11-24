//! Switch and Checkbox hand `on_click` the value they flip to; SegmentedControl and Tabs hand it the clicked index;
//! a disabled toggle reports nothing.

use super::{Checkbox, SegmentedControl, Switch, Tabs};
use gpui::{
    Context, InteractiveElement as _, IntoElement, Modifiers, ParentElement as _, Render,
    Styled as _, TestAppContext, VisualTestContext, Window, div,
};
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct Seen(RefCell<Vec<String>>);

struct Harness {
    seen: Rc<Seen>,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let log = |tag: &'static str, seen: &Rc<Seen>| {
            let seen = seen.clone();
            move |v: String| seen.0.borrow_mut().push(format!("{tag}={v}"))
        };
        let (sw, cb, off, seg, tab) = (
            log("switch", &self.seen),
            log("check", &self.seen),
            log("off", &self.seen),
            log("seg", &self.seen),
            log("tab", &self.seen),
        );
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap_2()
            .p_4()
            .child(
                div().flex().debug_selector(|| "switch".into()).child(
                    Switch::new("sw")
                        .checked(false)
                        .on_click(move |v, _, _| sw(v.to_string())),
                ),
            )
            .child(
                div().flex().debug_selector(|| "check".into()).child(
                    Checkbox::new("cb")
                        .label("Delete branch")
                        .checked(true)
                        .on_click(move |v, _, _| cb(v.to_string())),
                ),
            )
            .child(
                div().flex().debug_selector(|| "off".into()).child(
                    Switch::new("off")
                        .disabled(true)
                        .on_click(move |v, _, _| off(v.to_string())),
                ),
            )
            .child(
                div().flex().debug_selector(|| "seg".into()).child(
                    SegmentedControl::new("seg")
                        .items(["One", "Two", "Three"])
                        .selected(Some(0))
                        .on_click(move |ix, _, _| seg(ix.to_string())),
                ),
            )
            .child(
                div().flex().debug_selector(|| "tabs".into()).child(
                    Tabs::new("tabs")
                        .items(["A", "B"])
                        .selected(Some(0))
                        .on_click(move |ix, _, _| tab(ix.to_string())),
                ),
            )
    }
}

fn rig(cx: &mut TestAppContext) -> (Rc<Seen>, &mut VisualTestContext) {
    super::button_test::setup(cx);
    let seen = Rc::new(Seen::default());
    let s = seen.clone();
    let (_, cx) = cx.add_window_view(move |_, _| Harness { seen: s });
    cx.run_until_parked();
    (seen, cx)
}

fn click_at(sel: &'static str, frac_x: f32, cx: &mut VisualTestContext) {
    let b = cx.debug_bounds(sel).expect("drawn");
    let at = gpui::point(b.origin.x + b.size.width * frac_x, b.center().y);
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn toggles_report_the_value_they_flip_to(cx: &mut TestAppContext) {
    let (seen, cx) = rig(cx);
    click_at("switch", 0.5, cx);
    click_at("check", 0.1, cx);
    click_at("off", 0.5, cx);
    assert_eq!(*seen.0.borrow(), ["switch=true", "check=false"]);
}

#[gpui::test]
fn pickers_report_the_clicked_index(cx: &mut TestAppContext) {
    let (seen, cx) = rig(cx);
    // The segments and tabs sit side by side; the last third and the right end hit the last item.
    click_at("seg", 0.85, cx);
    click_at("tabs", 0.02, cx);
    assert_eq!(seen.0.borrow()[0], "seg=2");
    assert_eq!(seen.0.borrow()[1], "tab=0");
}
