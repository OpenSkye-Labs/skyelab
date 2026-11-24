//! A Switch and a Checkbox are tab stops that Space flips, as gpui-component's were.

use super::{Checkbox, Switch};
use gpui::{
    Context, IntoElement, ParentElement as _, Render, Styled as _, TestAppContext, Window, div,
};
use std::{cell::RefCell, rc::Rc};

/// A real key press: GPUI's keyboard click fires on the key-up (`simulate_keystrokes` sends only the down).
fn press(key: &str, cx: &mut gpui::VisualTestContext) {
    let stroke = cx.update(|_, cx| {
        gpui::Keystroke::parse_with_os(key, cx.platform_info().os).expect("a plain key parses")
    });
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: stroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(gpui::KeyUpEvent { keystroke: stroke });
    cx.run_until_parked();
}

struct Harness {
    seen: Rc<RefCell<Vec<String>>>,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (a, b) = (self.seen.clone(), self.seen.clone());
        div()
            .flex()
            .flex_col()
            .p_4()
            .child(
                Switch::new("sw")
                    .accessibility_label("Switch")
                    .checked(false)
                    .on_click(move |v, _, _| a.borrow_mut().push(format!("switch={v}"))),
            )
            .child(
                Checkbox::new("cb")
                    .label("Check")
                    .checked(false)
                    .on_click(move |v, _, _| b.borrow_mut().push(format!("check={v}"))),
            )
    }
}

#[gpui::test]
fn toggles_are_tab_stops_that_space_flips(cx: &mut TestAppContext) {
    super::button_test::setup(cx);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s = seen.clone();
    let (_, cx) = cx.add_window_view(move |_, _| Harness { seen: s });
    cx.run_until_parked();

    cx.update(|window, cx| window.focus_next(cx));
    let focused = cx.update(|window, cx| window.focused(cx).is_some());
    assert!(
        focused,
        "toggle is not reachable by keyboard: Tab focused nothing"
    );
    press("space", cx);
    cx.update(|window, cx| window.focus_next(cx));
    press("enter", cx);
    assert_eq!(
        *seen.borrow(),
        ["switch=true", "check=true"],
        "Space/Enter did not flip the focused toggles"
    );
}
