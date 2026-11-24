//! A disabled toggle inside a clickable row keeps the click from reaching the row.

use super::Switch;
use gpui::{
    Context, InteractiveElement as _, IntoElement, Modifiers, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, TestAppContext, Window, div, px,
};
use std::{cell::Cell, rc::Rc};

struct Row {
    parent: Rc<Cell<usize>>,
}

impl Render for Row {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let p = self.parent.clone();
        div()
            .id("row")
            .size(px(200.))
            .p_4()
            .flex()
            .flex_col()
            .items_start()
            .on_click(move |_, _, _| p.set(p.get() + 1))
            .child(
                div()
                    .flex()
                    .debug_selector(|| "off".into())
                    .child(Switch::new("off").disabled(true)),
            )
    }
}

#[gpui::test]
fn a_disabled_switch_does_not_click_its_row(cx: &mut TestAppContext) {
    super::button_test::setup(cx);
    let parent = Rc::new(Cell::new(0));
    let p = parent.clone();
    let (_, cx) = cx.add_window_view(move |_, _| Row { parent: p });
    cx.run_until_parked();
    let at = cx.debug_bounds("off").expect("drawn").center();
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(parent.get(), 0, "DISABLED_TOGGLE_CLICK_BUBBLED");
}
