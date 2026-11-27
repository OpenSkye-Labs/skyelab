//! SkyeLab's theme: the colors, fonts and sizes every view paints with, read through [`ActiveTheme`]. Our loader
//! (`theme_load`) builds it from the family files in `themes/`; `themes::apply` installs it.

use gpui::{App, Global, Hsla, Pixels, SharedString, WindowAppearance};
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}

impl ThemeMode {
    pub fn is_dark(self) -> bool {
        self == ThemeMode::Dark
    }
}

impl From<WindowAppearance> for ThemeMode {
    fn from(a: WindowAppearance) -> Self {
        match a {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
            WindowAppearance::Light | WindowAppearance::VibrantLight => ThemeMode::Light,
        }
    }
}

/// What views paint with; `theme_load::resolve` maps the theme files' keys onto it.
#[derive(Debug, Clone)]
pub struct Theme {
    pub mode: ThemeMode,
    pub background: Hsla,
    pub foreground: Hsla,
    pub border: Hsla,
    pub muted: Hsla,
    pub muted_foreground: Hsla,
    pub accent: Hsla,
    pub accent_foreground: Hsla,
    pub primary: Hsla,
    pub primary_foreground: Hsla,
    pub primary_hover: Hsla,
    pub primary_active: Hsla,
    pub secondary: Hsla,
    pub secondary_foreground: Hsla,
    pub secondary_hover: Hsla,
    pub secondary_active: Hsla,
    pub danger: Hsla,
    pub danger_foreground: Hsla,
    pub danger_hover: Hsla,
    pub danger_active: Hsla,
    pub warning: Hsla,
    pub success: Hsla,
    pub info: Hsla,
    pub input: Hsla,
    pub popover: Hsla,
    pub popover_foreground: Hsla,
    pub ring: Hsla,
    pub selection: Hsla,
    pub overlay: Hsla,
    pub link: Hsla,
    pub tab_bar: Hsla,
    pub sidebar: Hsla,
    pub sidebar_foreground: Hsla,
    pub sidebar_accent: Hsla,
    pub sidebar_accent_foreground: Hsla,
    pub sidebar_border: Hsla,
    pub red: Hsla,
    pub green: Hsla,
    pub blue: Hsla,
    pub yellow: Hsla,
    pub magenta: Hsla,
    pub cyan: Hsla,
    pub chart_1: Hsla,
    pub chart_2: Hsla,
    pub chart_3: Hsla,
    pub chart_4: Hsla,
    pub chart_5: Hsla,
    pub font_family: SharedString,
    pub font_size: Pixels,
    pub mono_font_family: SharedString,
    pub mono_font_size: Pixels,
    /// Cards and code blocks.
    pub radius: Pixels,
    pub radius_lg: Pixels,
    /// gpui-component's highlighter still paints code, so its style travels with ours until the text stage.
    pub highlight_theme: Arc<gpui_component::highlighter::HighlightTheme>,
}

impl Theme {
    pub fn is_dark(&self) -> bool {
        self.mode.is_dark()
    }

    /// The installed theme; before `install_theme` (a test that doesn't need one), the default family's light theme.
    pub fn global(cx: &App) -> &Self {
        static DEFAULT: OnceLock<Theme> = OnceLock::new();
        cx.try_global::<Self>().unwrap_or_else(|| {
            DEFAULT.get_or_init(|| {
                super::theme_load::resolve(
                    super::themes::FAMILIES[0].json,
                    false,
                    &Default::default(),
                )
                .expect("the default theme file loads (theme_load_test)")
            })
        })
    }
}

impl Global for Theme {}

pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    #[inline]
    fn theme(&self) -> &Theme {
        Theme::global(self)
    }
}
