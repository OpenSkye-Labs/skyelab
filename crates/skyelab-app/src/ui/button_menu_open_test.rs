use super::*;

#[test]
fn a_selected_trigger_with_its_menu_closed_keeps_its_tooltip() {
    let b = Button::new("sidebar-scope")
        .selected(true)
        .tooltip("Projects");
    // What gpui-component's Popover does to its trigger on every frame while the menu is closed.
    let was_open = Selectable::is_open(&b);
    let b = Selectable::open(b, was_open);
    assert_eq!(
        b.menu_open,
        Some(false),
        "menu_open stays false when the menu is closed: tooltip kept"
    );
    assert!(
        !Selectable::is_open(&b),
        "is_open() stays false while the menu is closed"
    );
}
