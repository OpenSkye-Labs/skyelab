//! T3's key cap (`ui/kbd.tsx`): 20 px tall, `px-1`, radius 4, 12 px muted text on a 7% foreground fill. The text is
//! the platform's spelling of the keystroke (⌘K on macOS, Ctrl+K elsewhere).

use super::{META, RADIUS_SM};
use gpui::{
    Action, App, IntoElement, KeyContext, Keystroke, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, prelude::*, px,
};
use gpui_component::ActiveTheme as _;

#[derive(IntoElement, Clone)]
pub struct Kbd {
    stroke: Keystroke,
    /// Plain text, no cap: inside a menu row or a button, where the row is the box.
    bare: bool,
}

impl Kbd {
    pub fn new(stroke: Keystroke) -> Self {
        Self {
            stroke,
            bare: false,
        }
    }

    /// The key of the highest-precedence binding for `action` in `context` (`None`: anywhere); a chord shows its
    /// first stroke.
    pub fn for_action(action: &dyn Action, context: Option<&str>, window: &Window) -> Option<Self> {
        let binding = match context.and_then(|c| KeyContext::parse(c).ok()) {
            Some(context) => {
                window.highest_precedence_binding_for_action_in_context(action, context)
            }
            None => window.highest_precedence_binding_for_action(action),
        }?;
        Self::from_binding(&binding)
    }

    /// Like [`Self::for_action`], resolved where `focus` is.
    pub fn for_action_in(
        action: &dyn Action,
        focus: &gpui::FocusHandle,
        window: &Window,
    ) -> Option<Self> {
        Self::from_binding(&window.highest_precedence_binding_for_action_in(action, focus)?)
    }

    fn from_binding(binding: &gpui::KeyBinding) -> Option<Self> {
        let key = binding.keystrokes().first()?;
        Some(Self::new(Keystroke {
            modifiers: *key.modifiers(),
            key: key.key().into(),
            key_char: None,
        }))
    }

    pub fn bare(mut self) -> Self {
        self.bare = true;
        self
    }

    /// The keystroke as the platform writes it.
    pub fn label(&self) -> SharedString {
        gpui_component::kbd::Kbd::format(&self.stroke).into()
    }
}

impl RenderOnce for Kbd {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let label = self.label();
        let el = div()
            // The selector gpui-component's key cap recorded, kept for tests that look a drawn hint up.
            .debug_selector(|| format!("kbd:{}", self.stroke.unparse(cx.platform_info().os)))
            .flex_shrink_0()
            .text_size(META)
            .text_color(theme.muted_foreground)
            .line_height(gpui::relative(1.));
        if self.bare {
            return el.child(label);
        }
        el.flex()
            .items_center()
            .justify_center()
            .h(px(20.))
            .min_w(px(20.))
            .px_1()
            .rounded(RADIUS_SM)
            .bg(theme.foreground.opacity(0.07))
            .child(label)
    }
}
