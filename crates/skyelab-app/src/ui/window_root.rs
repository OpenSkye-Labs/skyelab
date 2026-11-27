//! Every SkyeLab window's root view: the window's content, then the layers our kit draws over it (dialogs; later menus
//! and toasts). Until gpui-component's `Root` goes (.agent/plans/2026-11-27-own-ui-kit.md, stage 8) it sits inside it.

use gpui::{
    AnyView, AppContext as _, Context, IntoElement, Render, Styled as _, Window, div, prelude::*,
};

pub struct WindowRoot {
    content: AnyView,
}

impl WindowRoot {
    /// What the window shows, under our layers.
    #[cfg(test)]
    pub fn content(&self) -> &AnyView {
        &self.content
    }
}

impl Render for WindowRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.content.clone())
    }
}

/// A window's root, as every window (tests included) builds it: gpui-component's `Root` around our [`WindowRoot`].
pub fn window_root(
    content: AnyView,
    window: &mut Window,
    cx: &mut Context<gpui_component::Root>,
) -> gpui_component::Root {
    let root = cx.new(|_| WindowRoot { content });
    gpui_component::Root::new(AnyView::from(root), window, cx)
}
