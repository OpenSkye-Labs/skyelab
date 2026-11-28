use crate::ui::{self, Button, PopupMenuItem};
use gpui::{
    AppContext as _, Context, IntoElement, Modifiers, ParentElement as _, Render, Role,
    Styled as _, TestAppContext, VisualTestContext, Window, div,
};
use gpui_base::test_support::snapshots;

struct Nested;

impl Render for Nested {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_4()
            .child(
                Button::new("trigger")
                    .label("Pick")
                    .dropdown_menu(|menu, window, cx| {
                        menu.item(PopupMenuItem::new("Alpha")).submenu(
                            "More",
                            window,
                            cx,
                            |menu, window, cx| {
                                menu.submenu("Deeper", window, cx, |menu, _, _| {
                                    menu.item(PopupMenuItem::new("Xi"))
                                })
                            },
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

fn center(vcx: &mut VisualTestContext, label: &str) -> gpui::Point<gpui::Pixels> {
    vcx.update(|window, _| snapshots(window))
        .into_iter()
        .find(|s| s.label() == Some(label) && s.visible())
        .unwrap_or_else(|| panic!("{label} drawn"))
        .bounds()
        .center()
}

#[gpui::test]
fn a_click_on_the_root_keeps_a_nested_submenu_chain_open(cx: &mut TestAppContext) {
    ui::button_test::setup(cx);
    let content = cx.update(|cx| cx.new(|_| Nested));
    let (_, vcx) =
        cx.add_window_view(move |window, cx| ui::window_root(content.clone().into(), window, cx));
    vcx.run_until_parked();
    let at = center(vcx, "Pick");
    vcx.simulate_mouse_move(at, None, Modifiers::none());
    vcx.simulate_click(at, Modifiers::none());
    vcx.run_until_parked();
    assert_eq!(menus(vcx), 1, "the trigger opens the menu");
    vcx.simulate_keystrokes("up right right");
    vcx.run_until_parked();
    assert_eq!(menus(vcx), 3, "both submenus open");
    let more = center(vcx, "More");
    vcx.simulate_click(more, Modifiers::none());
    vcx.run_until_parked();
    assert!(
        menus(vcx) > 0,
        "a click on the root's open submenu row keeps the menu open"
    );
}
