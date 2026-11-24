//! Following the phone's light or dark appearance.

use gpui::{App, Window};
#[cfg(target_os = "ios")]
use gpui_component::ActiveTheme as _;
use gpui_component::Theme;

/// The theme the phone's appearance asks for. Debug builds: `SKYELAB_VERIFY_THEME=dark|light` forces one, so a
/// screenshot can be taken in each without changing the device setting.
pub(super) fn follow_appearance(window: &mut Window, cx: &mut App) {
    let forced = match std::env::var("SKYELAB_VERIFY_THEME").as_deref() {
        Ok("dark") if cfg!(debug_assertions) => Some(gpui_component::ThemeMode::Dark),
        Ok("light") if cfg!(debug_assertions) => Some(gpui_component::ThemeMode::Light),
        _ => None,
    };
    match forced {
        Some(mode) => Theme::change(mode, Some(window), cx),
        None => Theme::sync_system_appearance(Some(window), cx),
    }
    // gpui_ios keeps dark status-bar text unless told: light text on the dark theme.
    #[cfg(target_os = "ios")]
    gpui_ios::ios::set_status_bar_style(if cx.theme().mode.is_dark() {
        gpui_ios::StatusBarContentStyle::Light
    } else {
        gpui_ios::StatusBarContentStyle::Dark
    });
}
