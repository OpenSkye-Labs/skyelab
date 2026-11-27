//! Every SkyeLab window's root view: the window's content, then the layers our kit draws over it (dialogs; later menus
//! and toasts). Until gpui-component's `Root` goes (.agent/plans/2026-11-27-own-ui-kit.md, stage 8) it sits inside it.

use super::ActiveTheme as _;
use super::dialog_layer::DialogLayer;
use gpui::{
    AnyView, AppContext as _, Context, Entity, IntoElement, Render, Styled as _, Window, div,
    prelude::*,
};

pub struct WindowRoot {
    content: AnyView,
    pub(super) dialogs: Entity<DialogLayer>,
}

impl WindowRoot {
    /// What the window shows, under our layers.
    #[cfg(test)]
    pub fn content(&self) -> &AnyView {
        &self.content
    }
}

impl Render for WindowRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        window.set_rem_size(theme.font_size);
        div()
            .size_full()
            .font_family(theme.font_family.clone())
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(self.content.clone())
            .child(self.dialogs.clone())
    }
}

/// A window's root, as every window (tests included) builds it: gpui-component's `Root` around our [`WindowRoot`].
pub fn window_root(
    content: AnyView,
    window: &mut Window,
    cx: &mut Context<gpui_component::Root>,
) -> gpui_component::Root {
    let root = cx.new(|cx| WindowRoot {
        content,
        dialogs: cx.new(|_| DialogLayer::new()),
    });
    gpui_component::Root::new(AnyView::from(root), window, cx)
}
