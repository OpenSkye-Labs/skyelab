//! Reads a theme family file into our [`Theme`](super::theme::Theme).

use super::theme::Theme;
use gpui::SharedString;
use skyelab_core::AppearanceSettings;

/// The light or dark theme in family file `json`, with the accent preset and text sizes from `s`.
pub fn resolve(_json: &str, _dark: bool, _s: &AppearanceSettings) -> anyhow::Result<Theme> {
    todo!("stage 2a: our theme loader")
}

/// `default` when installed, else the first installed of `alternates`, else the system UI font.
pub fn first_installed(
    _default: &str,
    _alternates: &[&str],
    _installed: &[String],
) -> SharedString {
    todo!("stage 2a: monospace fallback")
}
