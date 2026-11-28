use super::*;

#[test]
fn a_selected_trigger_with_its_menu_closed_keeps_its_tooltip() {
    let b = Button::new("sidebar-scope")
        .selected(true)
        .tooltip("Projects")
        .menu_open(false);
    assert_eq!(
        b.menu_open,
        Some(false),
        "menu_open stays false when the menu is closed: tooltip kept"
    );
    assert!(b.selected, "a selected trigger stays selected");
}
