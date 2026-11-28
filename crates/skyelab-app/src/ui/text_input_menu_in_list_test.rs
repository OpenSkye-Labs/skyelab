use super::{TextField, TextInput};
use gpui::{
    AppContext as _, Context, Entity, IntoElement, ListAlignment, ListState, Modifiers,
    MouseButton, ParentElement as _, Render, Styled as _, TestAppContext, Window, div, list, px,
};
use gpui_base::test_support::snapshots;

struct Page {
    input: Entity<TextInput>,
    rows: ListState,
}

impl Render for Page {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(px(14.));
        let input = self.input.clone();
        div().size(px(400.)).child(
            list(self.rows.clone(), move |_, _, _| {
                div()
                    .w(px(200.))
                    .child(TextField::new(&input))
                    .into_any_element()
            })
            .size_full(),
        )
    }
}

/// A question card's field sits in the timeline's list.
#[gpui::test]
fn the_menu_of_a_field_in_a_list_row_is_drawn(cx: &mut TestAppContext) {
    super::button_test::setup(cx);
    let (view, vcx) = cx.add_window_view(|window, cx| Page {
        input: cx.new(|cx| TextInput::new(window, cx).default_value("abc")),
        rows: ListState::new(1, ListAlignment::Top, px(100.)),
    });
    vcx.update(|window, _| window.activate_window());
    vcx.run_until_parked();
    let input = view.read_with(vcx, |p, _| p.input.clone());
    let at = input
        .read_with(vcx, |i, _| i.text_bounds())
        .expect("drawn")
        .center();
    vcx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    vcx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    vcx.run_until_parked();
    let labels: Vec<_> = vcx
        .update(|window, _| snapshots(window))
        .iter()
        .filter_map(|s| s.label().map(str::to_owned))
        .collect();
    for row in ["Cut", "Copy", "Paste", "Select All"] {
        assert!(
            labels.iter().any(|l| l == row),
            "MENU_NOT_DRAWN: {row} in {labels:?}"
        );
    }
}
