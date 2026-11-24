use super::*;

#[test]
fn a_selected_trigger_with_its_menu_closed_keeps_its_tooltip() {
    let b = Button::new("sidebar-scope")
        .selected(true)
        .tooltip("Projects");
    // What gpui-component's Popover does to its trigger on every frame while the menu is closed.
    let was_open = Selectable::is_open(&b);
    let b = Selectable::open(b, was_open);
    assert!(
        !b.menu_open,
        "menu_open set with the menu closed: tooltip gone"
    );
}
