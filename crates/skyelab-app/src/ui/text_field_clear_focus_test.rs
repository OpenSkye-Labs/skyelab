use super::{TextField, TextInput};
use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable as _, InteractiveElement as _,
    IntoElement, Modifiers, MouseButton, ParentElement as _, Render, Styled as _, TestAppContext,
    Window, div, point, px,
};

struct Page {
    outer: FocusHandle,
    input: Entity<TextInput>,
}

impl Render for Page {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(px(14.));
        div()
            .id("page")
            .track_focus(&self.outer)
            .size(px(300.))
            .child(
                div()
                    .w(px(200.))
                    .child(TextField::new(&self.input).cleanable(true)),
            )
    }
}

#[gpui::test]
fn a_press_on_the_x_keeps_the_field_focused(cx: &mut TestAppContext) {
    super::button_test::setup(cx);
    let (view, vcx) = cx.add_window_view(|window, cx| Page {
        outer: cx.focus_handle(),
        input: cx.new(|cx| TextInput::new(window, cx).default_value("abc")),
    });
    let input = view.read_with(vcx, |p, _| p.input.clone());
    let focus = input.read_with(vcx, |i, cx| i.focus_handle(cx));
    vcx.update(|window, cx| {
        window.activate_window();
        window.focus(&focus, cx);
    });
    vcx.run_until_parked();
    let text = input.read_with(vcx, |i, _| i.text_bounds()).expect("drawn");
    // The 6 px gap, then the middle of the 10.5 px icon.
    let x = point(text.right() + px(11.), text.center().y);
    vcx.simulate_mouse_down(x, MouseButton::Left, Modifiers::none());
    vcx.run_until_parked();
    let kept = vcx.update(|window, _| focus.is_focused(window));
    vcx.simulate_mouse_up(x, MouseButton::Left, Modifiers::none());
    assert_eq!(
        input.read_with(vcx, |i, _| i.value().to_string()),
        "",
        "missed the x"
    );
    assert!(kept, "pressing the x took the focus from the field");
}
