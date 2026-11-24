//! The sidebar's saved views button (see `saved_views.rs`).

use super::sidebar::ScopeMenu;
use super::*;
use gpui_component::Selectable as _;
use gpui_component::menu::DropdownMenu as _;
use gpui_kit_assets::IconName as Lucide;

/// The saved views menu (T3's `SavedViewsMenu`): each view (checked while the sidebar shows it), Save current view…,
/// and Delete view.
pub(super) fn views_button(scope: ScopeMenu, focus: &FocusHandle) -> impl IntoElement {
    use super::super::saved_views::{ApplySidebarView, DeleteSidebarView, SaveSidebarView};
    let focus = focus.clone();
    let shown = scope.views.iter().any(|(_, on)| *on);
    Button::new("sidebar-views")
        .ghost()
        .small()
        .icon(Icon::new(Lucide::Bookmark))
        .selected(shown)
        .tooltip("Saved views")
        .dropdown_menu(move |mut menu, window, cx| {
            menu = menu.action_context(focus.clone()).min_w(gpui::px(220.));
            for (name, on) in &scope.views {
                let pick = ApplySidebarView { name: name.clone() };
                menu = menu.menu_with_check(name.clone(), *on, Box::new(pick));
            }
            if !scope.views.is_empty() {
                menu = menu.separator();
            }
            menu = menu.menu_with_icon(
                "Save current view…",
                IconName::Plus,
                Box::new(SaveSidebarView),
            );
            if !scope.views.is_empty() {
                let names: Vec<SharedString> = scope.views.iter().map(|(n, _)| n.clone()).collect();
                menu = menu.submenu("Delete view", window, cx, move |mut menu, _, _| {
                    for name in &names {
                        let delete = DeleteSidebarView { name: name.clone() };
                        menu = menu.menu(name.clone(), Box::new(delete));
                    }
                    menu
                });
            }
            menu
        })
}
