use crate::ui::{self, Button, PopupMenuItem};
use gpui::{
    AppContext as _, Context, IntoElement, ParentElement as _, Render, Role, Styled as _,
    TestAppContext, VisualTestContext, Window, div,
};
use gpui_base::test_support::snapshots;
use std::{cell::RefCell, rc::Rc};

struct Harness(Rc<RefCell<Vec<&'static str>>>);

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let picks = self.0.clone();
        div()
            .size_full()
            .p_4()
            .child(
                Button::new("trigger")
                    .label("Pick")
                    .dropdown_menu(move |menu, _, _| {
                        let picks = picks.clone();
                        menu.item(
                            PopupMenuItem::new("Alpha")
                                .on_click(move |_, _, _| picks.borrow_mut().push("alpha")),
                        )
                    }),
            )
    }
}

fn menus(vcx: &mut VisualTestContext) -> usize {
    vcx.update(|window, _| snapshots(window))
        .iter()
        .filter(|s| s.role() == Some(Role::Menu) && s.visible())
        .count()
}

fn trigger_focused(vcx: &mut VisualTestContext) -> bool {
    vcx.update(|window, _| snapshots(window))
        .iter()
        .any(|s| s.label() == Some("Pick") && s.focused() == Some(true))
}

#[gpui::test]
fn a_focused_trigger_opens_its_menu_from_the_keyboard(cx: &mut TestAppContext) {
    ui::button_test::setup(cx);
    let picks = Rc::new(RefCell::new(Vec::new()));
    let content = cx.update(|cx| {
        let picks = picks.clone();
        cx.new(|_| Harness(picks))
    });
    let (_, vcx) =
        cx.add_window_view(move |window, cx| ui::window_root(content.clone().into(), window, cx));
    vcx.run_until_parked();
    vcx.simulate_keystrokes("tab");
    vcx.run_until_parked();
    assert!(trigger_focused(vcx), "Tab reaches the trigger");
    for key in ["enter", "space"] {
        vcx.simulate_keystrokes(key);
        vcx.run_until_parked();
        assert_eq!(menus(vcx), 1, "{key} on the trigger opens its menu");
        vcx.simulate_keystrokes("escape");
        vcx.run_until_parked();
        assert_eq!(menus(vcx), 0);
        assert!(
            trigger_focused(vcx),
            "Escape gives focus back to the trigger"
        );
    }
    vcx.simulate_keystrokes("enter down enter");
    vcx.run_until_parked();
    assert_eq!(*picks.borrow(), ["alpha"], "Enter picks the selected row");
}

/// An accessibility press (VoiceOver, agent-desktop's AXPress) runs the trigger's a11y Click listener, which GPUI's
/// test window never registers; `A11yPress` runs the same closure on the trigger's node.
#[gpui::test]
fn an_accessibility_press_opens_and_closes_the_menu(cx: &mut TestAppContext) {
    ui::button_test::setup(cx);
    let content = cx.update(|cx| cx.new(|_| Harness(Rc::default())));
    let (_, vcx) =
        cx.add_window_view(move |window, cx| ui::window_root(content.clone().into(), window, cx));
    vcx.run_until_parked();
    vcx.simulate_keystrokes("tab");
    vcx.run_until_parked();
    let trigger = vcx
        .update(|window, cx| window.focused(cx))
        .expect("Tab focuses the trigger");
    let press = |vcx: &mut VisualTestContext| {
        vcx.update(|window, cx| trigger.dispatch_action(&ui::button::A11yPress, window, cx));
        vcx.run_until_parked();
    };
    press(vcx);
    assert_eq!(menus(vcx), 1, "an accessibility press opens the menu");
    press(vcx);
    assert_eq!(menus(vcx), 0, "a second press closes it");
}
