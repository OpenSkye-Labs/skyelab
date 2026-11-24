//! Pressing a Checkbox keeps the text field being typed in focused, as a button press does.

use super::Checkbox;
use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable as _, InteractiveElement as _,
    IntoElement, Modifiers, ParentElement as _, Render, Styled as _, TestAppContext, Window, div,
    point, px,
};
use gpui_component::input::{Input, InputState};

struct Form {
    outer: FocusHandle,
    input: Entity<InputState>,
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.outer)
            .flex()
            .flex_col()
            .items_start()
            .gap_2()
            .p_4()
            .size(px(300.))
            .child(div().w(px(200.)).child(Input::new(&self.input)))
            .child(
                div()
                    .flex()
                    .debug_selector(|| "cb".into())
                    .child(Checkbox::new("cb").label("Run").on_click(|_, _, _| {})),
            )
    }
}

#[gpui::test]
fn a_checkbox_press_keeps_the_field_focused(cx: &mut TestAppContext) {
    super::button_test::setup(cx);
    let (view, cx) = cx.add_window_view(|window, cx| Form {
        outer: cx.focus_handle(),
        input: cx.new(|cx| InputState::new(window, cx)),
    });
    cx.run_until_parked();
    let focus = view.read_with(cx, |f, cx| f.input.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.run_until_parked();
    let b = cx.debug_bounds("cb").expect("drawn");
    cx.simulate_click(point(b.origin.x + px(6.), b.center().y), Modifiers::none());
    cx.run_until_parked();
    assert!(
        cx.update(|window, _| focus.is_focused(window)),
        "CHECKBOX_STOLE_FOCUS"
    );
}
