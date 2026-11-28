use crate::ui::{self, SkyeIcon};
use gpui::{
    Context, Entity, InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Render,
    Styled as _, TestAppContext, Window, div, px,
};
use gpui_component::menu::{PopupMenu as TheirMenu, PopupMenuItem as TheirItem};

gpui::actions!(menu_parity_test, [Act]);

const CASES: [&str; 11] = [
    "plain",
    "checks",
    "icon",
    "label",
    "disabled",
    "binding",
    "min_w",
    "min_w_over",
    "right",
    "submenu",
    "element",
];

fn theirs(case: &str, window: &mut Window, cx: &mut gpui::App) -> Entity<TheirMenu> {
    let case = case.to_owned();
    TheirMenu::build(window, cx, move |m, window, cx| match case.as_str() {
        "plain" => m
            .item(TheirItem::new("Copy message to the clipboard"))
            .item(TheirItem::new("Copy as plain text, no markdown")),
        "checks" => m
            .item(TheirItem::new("Opus 4.5 with extended thinking").checked(true))
            .item(TheirItem::new("Sonnet 4.5, the account default")),
        "icon" => m
            .menu_with_icon("Save current view…", SkyeIcon::Plus, Box::new(Act))
            .menu("Delete the saved view", Box::new(Act)),
        "label" => m
            .label("Permissions")
            .item(TheirItem::new("Ask before every command runs"))
            .separator()
            .item(TheirItem::new("Bypass every permission prompt")),
        "disabled" => m
            .item(TheirItem::new("Rename this terminal tab"))
            .item(TheirItem::new("Close this tab and its shell").disabled(true)),
        "binding" => m
            .menu("Run the bound action now", Box::new(Act))
            .menu("Settle this thread for now", Box::new(Act)),
        "min_w" => m
            .min_w(px(220.))
            .menu_with_check("All projects", true, Box::new(Act)),
        "min_w_over" => m.min_w(px(220.)).menu_with_check(
            "A project with a much longer name than fits",
            true,
            Box::new(Act),
        ),
        "right" => m
            .check_side(gpui_component::Side::Right)
            .item(TheirItem::new("Ask before every command runs").checked(true))
            .item(TheirItem::new("Bypass every permission prompt")),
        "submenu" => m.menu("Settle this thread for now", Box::new(Act)).submenu(
            "Snooze until tomorrow morning",
            window,
            cx,
            |m, _, _| m.menu("Tonight, at six in the evening", Box::new(Act)),
        ),
        _ => m.item(TheirItem::element(|_, _| {
            div()
                .w(px(300.))
                .py_0p5()
                .child("Ask before every command, with a note")
        })),
    })
}

fn ours(case: &str, window: &mut Window, cx: &mut gpui::App) -> Entity<ui::PopupMenu> {
    use ui::PopupMenuItem as Item;
    let case = case.to_owned();
    ui::PopupMenu::build(window, cx, move |m, window, cx| match case.as_str() {
        "plain" => m
            .item(Item::new("Copy message to the clipboard"))
            .item(Item::new("Copy as plain text, no markdown")),
        "checks" => m
            .item(Item::new("Opus 4.5 with extended thinking").checked(true))
            .item(Item::new("Sonnet 4.5, the account default")),
        "icon" => m
            .menu_with_icon("Save current view…", SkyeIcon::Plus, Box::new(Act))
            .menu("Delete the saved view", Box::new(Act)),
        "label" => m
            .label("Permissions")
            .item(Item::new("Ask before every command runs"))
            .separator()
            .item(Item::new("Bypass every permission prompt")),
        "disabled" => m
            .item(Item::new("Rename this terminal tab"))
            .item(Item::new("Close this tab and its shell").disabled(true)),
        "binding" => m
            .menu("Run the bound action now", Box::new(Act))
            .menu("Settle this thread for now", Box::new(Act)),
        "min_w" => m
            .min_w(px(220.))
            .menu_with_check("All projects", true, Box::new(Act)),
        "min_w_over" => m.min_w(px(220.)).menu_with_check(
            "A project with a much longer name than fits",
            true,
            Box::new(Act),
        ),
        "right" => m
            .checks_right()
            .item(Item::new("Ask before every command runs").checked(true))
            .item(Item::new("Bypass every permission prompt")),
        "submenu" => m.menu("Settle this thread for now", Box::new(Act)).submenu(
            "Snooze until tomorrow morning",
            window,
            cx,
            |m, _, _| m.menu("Tonight, at six in the evening", Box::new(Act)),
        ),
        _ => m.item(Item::element("Ask", |_, _| {
            div()
                .w(px(300.))
                .py_0p5()
                .child("Ask before every command, with a note")
        })),
    })
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

struct Pairs(Vec<(Entity<TheirMenu>, Entity<ui::PopupMenu>)>);

impl Render for Pairs {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut col = div().flex().flex_col().items_start().gap_2();
        for (i, (a, b)) in self.0.iter().enumerate() {
            col = col.child(
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap_2()
                    .child(
                        div()
                            .debug_selector(move || format!("theirs-{i}"))
                            .child(a.clone()),
                    )
                    .child(
                        div()
                            .debug_selector(move || format!("ours-{i}"))
                            .child(b.clone()),
                    ),
            );
        }
        col
    }
}

#[gpui::test]
fn our_menus_take_the_same_size_as_gpui_components(cx: &mut TestAppContext) {
    ui::button_test::setup(cx);
    cx.update(|cx| cx.bind_keys([KeyBinding::new("cmd-k", Act, None)]));
    let (_view, vcx) = cx.add_window_view(|window, cx| {
        Pairs(
            CASES
                .iter()
                .map(|c| (theirs(c, window, cx), ours(c, window, cx)))
                .collect(),
        )
    });
    vcx.run_until_parked();
    for (i, case) in CASES.iter().enumerate() {
        let a = vcx
            .debug_bounds(leak(format!("theirs-{i}")))
            .expect("theirs drawn");
        let b = vcx
            .debug_bounds(leak(format!("ours-{i}")))
            .expect("ours drawn");
        assert_eq!(a.size, b.size, "{case}");
    }
}
