use super::{Button, Size, Variant, icon_button};
use gpui::{
    AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    Modifiers, ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, Window,
    div, px,
};
use gpui_component::input::{Input, InputState};
use std::{cell::Cell, rc::Rc};

pub(super) fn setup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        super::install_theme(cx).expect("the bundled themes parse");
    });
}

const SELECTORS: [&str; 18] = [
    "v0s0", "v0s1", "v0s2", "v1s0", "v1s1", "v1s2", "v2s0", "v2s1", "v2s2", "v3s0", "v3s1", "v3s2",
    "v4s0", "v4s1", "v4s2", "v5s0", "v5s1", "v5s2",
];

struct Harness {
    clicks: Rc<Cell<usize>>,
    disabled: bool,
    loading: bool,
    input: Entity<InputState>,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        let mut col = div().flex().flex_col().items_start().gap_2().p_4().child(
            div().debug_selector(|| "btn".into()).child(
                Button::new("btn")
                    .label("Push")
                    .disabled(self.disabled)
                    .loading(self.loading)
                    .on_click(move |_, _, _| clicks.set(clicks.get() + 1)),
            ),
        );
        col = col.child(div().w(px(200.)).child(Input::new(&self.input)));
        for (vi, v) in [
            Variant::Primary,
            Variant::Secondary,
            Variant::Ghost,
            Variant::Outline,
            Variant::Danger,
            Variant::Warning,
        ]
        .into_iter()
        .enumerate()
        {
            for (si, s) in [Size::Xs, Size::Sm, Size::Md].into_iter().enumerate() {
                let sel = SELECTORS[vi * 3 + si];
                col = col.child(
                    div().flex().debug_selector(move || sel.into()).child(
                        Button::new(("v", vi * 3 + si))
                            .variant(v)
                            .with_size(s)
                            .label("Label"),
                    ),
                );
            }
        }
        col.child(
            div()
                .flex()
                .debug_selector(|| "square".into())
                .child(icon_button("sq", super::SkyeIcon::Copy).with_size(Size::Sm)),
        )
    }
}

fn rig(
    cx: &mut TestAppContext,
    disabled: bool,
    loading: bool,
) -> (Entity<Harness>, Rc<Cell<usize>>, &mut VisualTestContext) {
    setup(cx);
    let clicks = Rc::new(Cell::new(0));
    let c = clicks.clone();
    let (view, cx) = cx.add_window_view(move |window, cx| Harness {
        clicks: c,
        disabled,
        loading,
        input: cx.new(|cx| InputState::new(window, cx)),
    });
    cx.run_until_parked();
    (view, clicks, cx)
}

fn click(sel: &'static str, cx: &mut VisualTestContext) {
    let at = cx.debug_bounds(sel).expect("drawn").center();
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn a_click_reaches_on_click(cx: &mut TestAppContext) {
    let (_, clicks, cx) = rig(cx, false, false);
    click("btn", cx);
    assert_eq!(clicks.get(), 1);
}

#[gpui::test]
fn a_disabled_button_swallows_clicks(cx: &mut TestAppContext) {
    let (_, clicks, cx) = rig(cx, true, false);
    click("btn", cx);
    assert_eq!(clicks.get(), 0);
}

#[gpui::test]
fn a_loading_button_swallows_clicks(cx: &mut TestAppContext) {
    let (_, clicks, cx) = rig(cx, false, true);
    click("btn", cx);
    assert_eq!(clicks.get(), 0);
}

#[gpui::test]
fn a_press_keeps_the_focused_field_focused(cx: &mut TestAppContext) {
    let (view, clicks, cx) = rig(cx, false, false);
    let focus = view.read_with(cx, |h, cx| h.input.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.run_until_parked();
    click("btn", cx);
    assert_eq!(clicks.get(), 1);
    assert!(
        cx.update(|window, _| focus.is_focused(window)),
        "the field still has focus after the button press"
    );
}

#[gpui::test]
fn every_variant_and_size_draws_at_its_height(cx: &mut TestAppContext) {
    let (_, _, cx) = rig(cx, false, false);
    for vi in 0..6 {
        for (si, s) in [Size::Xs, Size::Sm, Size::Md].into_iter().enumerate() {
            let b = cx.debug_bounds(SELECTORS[vi * 3 + si]).expect("drawn");
            assert_eq!(b.size.height, s.height(), "variant {vi} size {si}");
        }
    }
    let sq = cx.debug_bounds("square").expect("drawn");
    assert_eq!(sq.size.width, sq.size.height, "an icon button is square");
    assert_eq!(sq.size.height, Size::Sm.height());
}
