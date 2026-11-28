use crate::ui::{self, Button, HoverCard, Popover};
use gpui::{
    AppContext as _, Context, InteractiveElement as _, IntoElement, Modifiers, ParentElement as _,
    Point, Render, Styled as _, TestAppContext, VisualTestContext, Window, div, point, px,
};
use gpui_base::test_support::{ElementSnapshot, snapshots};
use std::time::Duration;

struct Harness {
    focus: gpui::FocusHandle,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .size_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                Popover::new("pop")
                    .trigger(|open| Button::new("pop-trigger").label("Open").selected(open))
                    .content(|_, _| Button::new("inside").label("Inside").on_click(|_, _, _| {})),
            )
            .child(
                div().mt(px(200.)).child(
                    HoverCard::new("card")
                        .open_delay(Duration::from_millis(500))
                        .trigger(
                            div()
                                .debug_selector(|| "hover-trigger".into())
                                .w(px(120.))
                                .h(px(24.))
                                .child("Hover me"),
                        )
                        .content(|_, _| {
                            div()
                                .debug_selector(|| "card".into())
                                .w(px(160.))
                                .h(px(60.))
                                .child("Card text")
                        }),
                ),
            )
    }
}

fn window(cx: &mut TestAppContext) -> (gpui::FocusHandle, &mut VisualTestContext) {
    ui::button_test::setup(cx);
    let focus = cx.update(|cx| cx.focus_handle());
    let content = cx.update(|cx| {
        let focus = focus.clone();
        cx.new(|_| Harness { focus })
    });
    let (_, vcx) =
        cx.add_window_view(move |window, cx| ui::window_root(content.clone().into(), window, cx));
    vcx.update(|window, cx| focus.focus(window, cx));
    draw(vcx);
    (focus, vcx)
}

fn draw(vcx: &mut VisualTestContext) {
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.draw(cx).clear(cx);
    });
}

fn node(vcx: &mut VisualTestContext, label: &str) -> Option<ElementSnapshot> {
    vcx.update(|window, _| snapshots(window))
        .into_iter()
        .find(|s| s.label() == Some(label) && s.visible())
}

fn click(vcx: &mut VisualTestContext, at: Point<gpui::Pixels>) {
    vcx.simulate_mouse_move(at, None, Modifiers::none());
    vcx.simulate_click(at, Modifiers::none());
    draw(vcx);
}

fn trigger(vcx: &mut VisualTestContext) -> Point<gpui::Pixels> {
    node(vcx, "Open").expect("trigger drawn").bounds().center()
}

#[gpui::test]
fn a_popover_toggles_and_closes_on_escape_or_outside(cx: &mut TestAppContext) {
    let (focus, vcx) = window(cx);
    assert!(node(vcx, "Inside").is_none());
    let at = trigger(vcx);
    click(vcx, at);
    assert!(
        node(vcx, "Inside").is_some(),
        "a click on the trigger opens it"
    );
    click(vcx, at);
    assert!(node(vcx, "Inside").is_none(), "a second click closes it");

    click(vcx, at);
    vcx.simulate_keystrokes("escape");
    draw(vcx);
    assert!(node(vcx, "Inside").is_none(), "Escape closes it");
    assert!(
        vcx.update(|window, _| focus.is_focused(window)),
        "focus is back where it was"
    );

    click(vcx, at);
    click(vcx, point(px(600.), px(500.)));
    assert!(node(vcx, "Inside").is_none(), "a press outside closes it");
}

fn advance(vcx: &mut VisualTestContext, ms: u64) {
    vcx.executor().advance_clock(Duration::from_millis(ms));
    draw(vcx);
}

fn card_shown(vcx: &mut VisualTestContext) -> bool {
    vcx.debug_bounds("card").is_some()
}

#[gpui::test]
fn a_hover_card_waits_then_stays_while_hovered(cx: &mut TestAppContext) {
    let (_, vcx) = window(cx);
    let at = vcx
        .debug_bounds("hover-trigger")
        .expect("trigger drawn")
        .center();
    vcx.simulate_mouse_move(at, None, Modifiers::none());
    advance(vcx, 400);
    assert!(!card_shown(vcx), "not before the open delay");
    advance(vcx, 200);
    assert!(card_shown(vcx), "shown after the open delay");

    let on_card = vcx.debug_bounds("card").expect("card drawn").center();
    vcx.simulate_mouse_move(on_card, None, Modifiers::none());
    advance(vcx, 1000);
    assert!(card_shown(vcx), "the pointer on the card keeps it open");

    vcx.simulate_mouse_move(point(px(600.), px(500.)), None, Modifiers::none());
    advance(vcx, 1000);
    assert!(
        !card_shown(vcx),
        "gone a second after the pointer leaves both"
    );
}
