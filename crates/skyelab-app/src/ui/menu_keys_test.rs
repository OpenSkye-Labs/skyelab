use crate::ui::{self, Button, PopupMenuItem};
use gpui::{
    AppContext as _, ClickEvent, Context, FocusHandle, InteractiveElement as _, IntoElement,
    KeyBinding, Modifiers, ParentElement as _, Point, Render, Role, Styled as _, TestAppContext,
    VisualTestContext, Window, div, point, px,
};
use gpui_base::test_support::{ElementSnapshot, snapshots};
use std::{cell::RefCell, rc::Rc};

gpui::actions!(menu_keys_test, [Act]);

type Picks = Rc<RefCell<Vec<&'static str>>>;

struct Harness {
    picks: Picks,
    focus: FocusHandle,
}

fn pick(
    picks: &Picks,
    name: &'static str,
) -> impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + use<> {
    let picks = picks.clone();
    move |_, _, _| picks.borrow_mut().push(name)
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (picks, focus) = (self.picks.clone(), self.focus.clone());
        div()
            .size_full()
            .p_4()
            .key_context("MenuKeysTest")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Act, _, _| this.picks.borrow_mut().push("act")))
            .child(
                Button::new("trigger")
                    .label("Pick")
                    .dropdown_menu(move |menu, window, cx| {
                        let sub = picks.clone();
                        menu.action_context(focus.clone())
                            .label("Section")
                            .item(PopupMenuItem::new("Alpha").on_click(pick(&picks, "alpha")))
                            .separator()
                            .item(
                                PopupMenuItem::new("Beta")
                                    .disabled(true)
                                    .on_click(pick(&picks, "beta")),
                            )
                            .item(
                                PopupMenuItem::new("Gamma")
                                    .checked(true)
                                    .on_click(pick(&picks, "gamma")),
                            )
                            .menu_with_check("Delta", false, Box::new(Act))
                            .item(PopupMenuItem::element("Epsilon", |_, _| {
                                div().child("Epsilon, with a note under it")
                            }))
                            .submenu("More", window, cx, move |menu, _, _| {
                                menu.item(PopupMenuItem::new("Xi").on_click(pick(&sub, "xi")))
                            })
                    }),
            )
    }
}

fn window(cx: &mut TestAppContext) -> (Picks, FocusHandle, &mut VisualTestContext) {
    ui::button_test::setup(cx);
    cx.update(|cx| cx.bind_keys([KeyBinding::new("cmd-k", Act, Some("MenuKeysTest"))]));
    let picks = Picks::default();
    let focus = cx.update(|cx| cx.focus_handle());
    let content = cx.update(|cx| {
        let (picks, focus) = (picks.clone(), focus.clone());
        cx.new(|_| Harness { picks, focus })
    });
    let (_, vcx) =
        cx.add_window_view(move |window, cx| ui::window_root(content.clone().into(), window, cx));
    vcx.run_until_parked();
    vcx.update(|window, cx| focus.focus(window, cx));
    vcx.run_until_parked();
    (picks, focus, vcx)
}

fn tree(vcx: &mut VisualTestContext) -> Vec<ElementSnapshot> {
    vcx.update(|window, _| snapshots(window))
}

fn node(vcx: &mut VisualTestContext, label: &str) -> Option<ElementSnapshot> {
    tree(vcx)
        .into_iter()
        .find(|s| s.label() == Some(label) && s.visible())
}

fn menus(vcx: &mut VisualTestContext) -> usize {
    tree(vcx)
        .iter()
        .filter(|s| s.role() == Some(Role::Menu) && s.visible())
        .count()
}

fn selected(vcx: &mut VisualTestContext, label: &str) -> bool {
    node(vcx, label).and_then(|s| s.selected()) == Some(true)
}

fn click(vcx: &mut VisualTestContext, at: Point<gpui::Pixels>) {
    vcx.simulate_mouse_move(at, None, Modifiers::none());
    vcx.simulate_click(at, Modifiers::none());
    vcx.run_until_parked();
}

fn open(vcx: &mut VisualTestContext) {
    let at = node(vcx, "Pick").expect("trigger drawn").bounds().center();
    click(vcx, at);
    assert_eq!(menus(vcx), 1, "a click on the trigger opens one menu");
}

fn keys(vcx: &mut VisualTestContext, keys: &str) {
    vcx.simulate_keystrokes(keys);
    vcx.run_until_parked();
}

/// Whether focus is back on what held it before the menu opened.
fn focus_back(vcx: &mut VisualTestContext, focus: &FocusHandle) -> bool {
    vcx.update(|window, _| focus.is_focused(window))
}

#[gpui::test]
fn a_click_opens_a_named_menu(cx: &mut TestAppContext) {
    let (_, _, vcx) = window(cx);
    assert_eq!(menus(vcx), 0);
    assert_eq!(node(vcx, "Pick").and_then(|s| s.expanded()), Some(false));
    open(vcx);
    assert_eq!(node(vcx, "Pick").and_then(|s| s.expanded()), Some(true));
    for label in ["Alpha", "Beta", "Gamma", "Delta", "Epsilon", "More"] {
        let row = node(vcx, label).unwrap_or_else(|| panic!("a row named {label}"));
        assert_eq!(row.role(), Some(Role::MenuItem), "{label} is a menu item");
    }
    assert_eq!(node(vcx, "Beta").and_then(|s| s.disabled()), Some(true));
    assert_eq!(node(vcx, "Gamma").and_then(|s| s.checked()), Some(true));
    assert_eq!(node(vcx, "Delta").and_then(|s| s.checked()), Some(false));
    assert_eq!(node(vcx, "Alpha").and_then(|s| s.checked()), None);
}

#[gpui::test]
fn arrows_skip_rows_that_cannot_be_picked(cx: &mut TestAppContext) {
    let (picks, focus, vcx) = window(cx);
    open(vcx);
    keys(vcx, "down");
    assert!(selected(vcx, "Alpha"), "the label is skipped");
    keys(vcx, "down");
    assert!(
        selected(vcx, "Gamma"),
        "the separator and the disabled row are skipped"
    );
    keys(vcx, "up");
    assert!(selected(vcx, "Alpha"));
    keys(vcx, "enter");
    assert_eq!(*picks.borrow(), ["alpha"]);
    assert_eq!(menus(vcx), 0, "a pick closes the menu");
    assert!(focus_back(vcx, &focus), "focus is back where it was");
    assert_eq!(node(vcx, "Pick").and_then(|s| s.expanded()), Some(false));
}

#[gpui::test]
fn escape_and_an_outside_press_close_without_picking(cx: &mut TestAppContext) {
    let (picks, focus, vcx) = window(cx);
    open(vcx);
    keys(vcx, "escape");
    assert_eq!(menus(vcx), 0, "Escape closes the menu");
    assert!(focus_back(vcx, &focus), "focus is back where it was");
    open(vcx);
    click(vcx, point(px(4.), px(4.)));
    assert_eq!(menus(vcx), 0, "a press outside closes the menu");
    assert!(picks.borrow().is_empty(), "nothing was picked");
}

#[gpui::test]
fn an_action_row_reaches_the_action_context(cx: &mut TestAppContext) {
    let (picks, _, vcx) = window(cx);
    open(vcx);
    let at = node(vcx, "Delta").expect("Delta drawn").bounds().center();
    click(vcx, at);
    assert_eq!(
        *picks.borrow(),
        ["act"],
        "the action ran once, in the action context"
    );
    assert_eq!(menus(vcx), 0);
}

#[gpui::test]
fn hover_selects_and_a_click_picks(cx: &mut TestAppContext) {
    let (picks, _, vcx) = window(cx);
    open(vcx);
    keys(vcx, "down");
    let alpha = node(vcx, "Alpha").expect("Alpha drawn").bounds().center();
    let gamma = node(vcx, "Gamma").expect("Gamma drawn").bounds().center();
    vcx.simulate_click(gamma, Modifiers::none());
    vcx.run_until_parked();
    assert_eq!(
        *picks.borrow(),
        ["gamma"],
        "a click picks the row under it, not the selected one"
    );
    picks.borrow_mut().clear();
    open(vcx);
    vcx.simulate_mouse_move(alpha, None, Modifiers::none());
    vcx.run_until_parked();
    let at = node(vcx, "Gamma").expect("Gamma drawn").bounds().center();
    vcx.simulate_mouse_move(at, None, Modifiers::none());
    vcx.run_until_parked();
    assert!(
        selected(vcx, "Gamma"),
        "the row under the pointer is selected"
    );
    click(vcx, at);
    assert_eq!(*picks.borrow(), ["gamma"]);
    assert_eq!(menus(vcx), 0);
}

#[gpui::test]
fn right_and_left_walk_the_submenu(cx: &mut TestAppContext) {
    let (picks, _, vcx) = window(cx);
    open(vcx);
    keys(vcx, "up");
    assert!(
        selected(vcx, "More"),
        "Up with no row selected selects the last"
    );
    keys(vcx, "right");
    assert_eq!(menus(vcx), 2, "Right opens the submenu");
    assert!(node(vcx, "Xi").is_some());
    keys(vcx, "left");
    assert_eq!(menus(vcx), 1, "Left closes only the submenu");
    assert!(selected(vcx, "More"));
    keys(vcx, "right");
    keys(vcx, "escape");
    assert_eq!(menus(vcx), 0, "Escape in the submenu closes both");
    open(vcx);
    keys(vcx, "up right");
    keys(vcx, "down enter");
    assert_eq!(*picks.borrow(), ["xi"]);
    assert_eq!(menus(vcx), 0, "a pick in the submenu closes both");
}

struct Bare {
    focus: FocusHandle,
    menu: Option<gpui::Entity<ui::PopupMenu>>,
}

impl Render for Bare {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .track_focus(&self.focus)
            .children(self.menu.clone())
    }
}

#[gpui::test]
fn a_menu_opened_on_its_own_gives_focus_back(cx: &mut TestAppContext) {
    ui::button_test::setup(cx);
    let (view, vcx) = cx.add_window_view(|_, cx| Bare {
        focus: cx.focus_handle(),
        menu: None,
    });
    let focus = vcx.update(|window, cx| {
        let focus = view.read(cx).focus.clone();
        focus.focus(window, cx);
        focus
    });
    vcx.update(|window, cx| {
        let menu = ui::PopupMenu::build(window, cx, |m, _, _| m.item(PopupMenuItem::new("Alpha")));
        gpui::Focusable::focus_handle(menu.read(cx), cx).focus(window, cx);
        view.update(cx, |v, _| v.menu = Some(menu));
    });
    vcx.run_until_parked();
    keys(vcx, "escape");
    assert!(
        focus_back(vcx, &focus),
        "Escape gives focus back to what had it"
    );
}
