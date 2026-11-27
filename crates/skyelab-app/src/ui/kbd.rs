//! T3's key cap (`ui/kbd.tsx`): 20 px tall, `px-1`, radius 4, 12 px muted text on a 7% foreground fill. The text is
//! the platform's spelling of the keystroke (⌘K on macOS, Ctrl+K elsewhere).

use super::{META, RADIUS_SM};
use crate::ui::ActiveTheme as _;
use gpui::{
    Action, App, IntoElement, KeyContext, Keystroke, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, prelude::*, px,
};

#[derive(IntoElement, Clone)]
pub struct Kbd {
    stroke: Keystroke,
    /// Plain text, no cap: inside a menu row or a button, where the row is the box.
    bare: bool,
    /// Words instead of the platform's symbol (T3's palette footer reads "Enter", not ↩).
    text: Option<SharedString>,
}

impl Kbd {
    pub fn new(stroke: Keystroke) -> Self {
        Self {
            stroke,
            bare: false,
            text: None,
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

    pub fn with_label(mut self, text: impl Into<SharedString>) -> Self {
        self.text = Some(text.into());
        self
    }

    pub fn bare(mut self) -> Self {
        self.bare = true;
        self
    }

    /// The keystroke as the platform writes it, except Escape: macOS's ⎋ reads as a refresh arrow at 12 px, so it
    /// is "Esc" as in T3.
    pub fn label(&self) -> SharedString {
        if let Some(text) = &self.text {
            return text.clone();
        }
        gpui_component::kbd::Kbd::format(&self.stroke)
            .replace('⎋', "Esc")
            .into()
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

/// A shortcut written in macOS glyphs (`"⌘⇧↩"`), spelled for this platform: unchanged on macOS, `Ctrl+Shift+Enter`
/// elsewhere. For tooltips and hints that name a key in running text.
pub fn keys(mac: &str) -> String {
    keys_on(mac, cfg!(target_os = "macos"))
}

fn keys_on(mac: &str, is_mac: bool) -> String {
    if is_mac {
        return mac.to_owned();
    }
    mac.chars()
        .map(|c| match c {
            '⌘' | '⌃' => "Ctrl".to_owned(),
            '⇧' => "Shift".to_owned(),
            '⌥' => "Alt".to_owned(),
            '↩' => "Enter".to_owned(),
            c => c.to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
#[test]
fn keys_are_spelled_per_platform() {
    assert_eq!(keys_on("⌘⇧↩", true), "⌘⇧↩");
    assert_eq!(keys_on("⌘⇧↩", false), "Ctrl+Shift+Enter");
    assert_eq!(keys_on("⌘.", false), "Ctrl+.");
    assert_eq!(keys_on("⌘N", false), "Ctrl+N");
}
