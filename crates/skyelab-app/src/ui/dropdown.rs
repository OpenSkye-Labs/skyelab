use super::popover::Popover;
use super::{Button, PopupMenu};
use gpui::{
    Anchor, App, Context, ElementId, Focusable as _, IntoElement, RenderOnce, SharedString, Window,
};
use std::rc::Rc;

type Build = Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

#[derive(IntoElement)]
pub struct DropdownMenu {
    button: Button,
    anchor: Anchor,
    build: Build,
}

impl Button {
    /// Opens the menu `build` fills, below the button's left edge. The menu is built each time it opens.
    pub fn dropdown_menu(
        self,
        build: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> DropdownMenu {
        self.dropdown_menu_with_anchor(Anchor::TopLeft, build)
    }

    /// Like [`Self::dropdown_menu`], with the menu's `anchor` corner against the button.
    pub fn dropdown_menu_with_anchor(
        self,
        anchor: Anchor,
        build: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> DropdownMenu {
        DropdownMenu {
            button: self,
            anchor,
            build: Rc::new(build),
        }
    }
}

impl RenderOnce for DropdownMenu {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let id: ElementId =
            SharedString::from(format!("dropdown-menu:{:?}", self.button.element_id())).into();
        let (button, build) = (self.button, self.build);
        Popover::new(id)
            .anchor(self.anchor)
            .bare()
            .trigger(move |open| button.menu_open(open))
            .content_with_state(move |state, window, cx| {
                if let Some(menu) = state.read(cx).menu.clone() {
                    return menu.into_any_element();
                }
                let build = build.clone();
                let menu = PopupMenu::build(window, cx, move |m, window, cx| build(m, window, cx));
                menu.focus_handle(cx).focus(window, cx);
                state.update(cx, |s, cx| s.hold_menu(menu.clone(), window, cx));
                menu.into_any_element()
            })
    }
}
