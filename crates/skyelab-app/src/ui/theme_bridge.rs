//! The one writer of gpui-component's theme, which that crate's widgets still in use read (inputs, menus, the dock,
//! the text view, the terminal and browser crates). It gets the same adjusted theme files as our loader, so both
//! resolve the same colors (`theme_match_test`), and our motion and caret blink. It goes with gpui-component.

use super::motion::{FADE, FADE_FAST, SPRING, ease_out_quad_bezier};
use gpui::App;
use gpui_component::theme::MotionTokens;
use gpui_component::{Theme, ThemeConfig, ThemeMode};
use serde_json::Value;
use std::rc::Rc;
use std::time::Duration;

/// Installs the adjusted `[light, dark]` theme objects and shows `dark` or light.
pub(super) fn write(themes: [Value; 2], dark: bool, cx: &mut App) {
    let [light, dark_theme] = themes.map(|t| {
        serde_json::from_value::<ThemeConfig>(t)
            .map_err(|e| tracing::error!("gpui-component can't read a theme: {e}"))
            .ok()
    });
    let (Some(light), Some(dark_theme)) = (light, dark_theme) else {
        return;
    };
    let theme = Theme::global_mut(cx);
    theme.light_theme = Rc::new(light);
    theme.dark_theme = Rc::new(dark_theme);
    theme.cursor_blink = super::CURSOR_BLINK;
    theme.motion = motion_tokens();
    let mode = if dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    Theme::change(mode, None, cx);
}

/// gpui-component's motion tokens set to Rex's values, so its own pieces (dock tab bars, resize handles) move like
/// ours.
pub(super) fn motion_tokens() -> MotionTokens {
    let mut m = MotionTokens::default();
    m.duration_fast = FADE_FAST;
    m.duration_normal = FADE;
    m.duration_slow = Duration::from_millis(250);
    m.easing_enter = ease_out_quad_bezier();
    m.easing_move = ease_out_quad_bezier();
    m.spring_control = SPRING;
    m.spring_move = SPRING.with_epsilon(0.1);
    m
}
