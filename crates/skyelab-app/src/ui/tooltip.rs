//! T3's tooltip (`ui/tooltip.tsx`): radius 6, hairline border, popover fill, 12 px text, `px-2 py-1`, and the key that
//! runs the same action when there is one. GPUI draws a tooltip as a view, so this is the one component that is an
//! entity: built only when the pointer rests on the trigger, never per frame.

use super::{Kbd, META, RADIUS_MD};
use gpui::{
    Action, AnyView, App, AppContext as _, Context, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::{ActiveTheme as _, h_flex};

pub struct Tooltip {
    text: SharedString,
    action: Option<(Box<dyn Action>, Option<SharedString>)>,
}

impl Tooltip {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            action: None,
        }
    }

    /// Shows the key bound to `action` (in `context`) beside the text.
    pub fn action(mut self, action: &dyn Action, context: Option<&str>) -> Self {
        self.action = Some((action.boxed_clone(), context.map(SharedString::new)));
        self
    }

    pub fn build(self, _: &mut Window, cx: &mut App) -> AnyView {
        cx.new(|_| self).into()
    }

    /// For `.tooltip(..)` on any stateful element: `div().id(..).tooltip(Tooltip::text("Copy"))`.
    pub fn text(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
        let text = text.into();
        move |window, cx| Tooltip::new(text.clone()).build(window, cx)
    }
}

impl Render for Tooltip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let key = self
            .action
            .as_ref()
            .and_then(|(a, c)| Kbd::for_action(a.as_ref(), c.as_deref(), window));
        let theme = cx.theme();
        // The margin keeps the box off the pointer; GPUI places the view at the cursor.
        div().child(
            h_flex()
                .m_3()
                .max_w(gpui::px(360.))
                .gap_2()
                .px_2()
                .py_1()
                .rounded(RADIUS_MD)
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .shadow_md()
                .font_family(theme.font_family.clone())
                .text_size(META)
                .text_color(theme.popover_foreground)
                .child(div().min_w_0().child(self.text.clone()))
                .when_some(key, |this, key| this.child(key)),
        )
    }
}
