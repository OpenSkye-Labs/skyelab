//! Reads a theme family file into our [`Theme`]: its light or dark theme with the user's accent and sizes written in
//! (`themes::adjust`), every color in hex.

use super::theme::{Theme, ThemeMode};
use anyhow::{Context as _, anyhow, bail};
use gpui::{App, Hsla, SharedString, font, px};
use gpui_component::highlighter::{HighlightTheme, HighlightThemeStyle};
use serde_json::Value;
use skyelab_core::AppearanceSettings;
use std::sync::{Arc, OnceLock};

const SYSTEM_UI_FONT: &str = ".SystemUIFont";

/// The platform's monospace family, then what to try when it isn't installed.
const MONO: (&str, &[&str]) = if cfg!(target_os = "macos") {
    ("Menlo", &["Monaco", "Courier New"])
} else if cfg!(target_os = "windows") {
    ("Consolas", &["Cascadia Mono", "Courier New"])
} else {
    (
        "DejaVu Sans Mono",
        &["Noto Sans Mono", "Liberation Mono", "Ubuntu Mono"],
    )
};

/// The light or dark theme in family file `json`, with the accent preset and text sizes from `s`.
pub fn resolve(json: &str, dark: bool, s: &AppearanceSettings) -> anyhow::Result<Theme> {
    let file: Value = serde_json::from_str(json).context("theme file is not JSON")?;
    let mut theme = pick(&file, dark)?.clone();
    super::themes::adjust(&mut theme, s, dark);
    from_value(&theme, dark)
}

/// The theme object for the mode in a family file.
pub(super) fn pick(file: &Value, dark: bool) -> anyhow::Result<&Value> {
    let mode = if dark { "dark" } else { "light" };
    file["themes"]
        .as_array()
        .and_then(|list| list.iter().find(|t| t["mode"] == mode))
        .ok_or_else(|| anyhow!("no {mode} theme"))
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa`.
pub(crate) fn parse_color(s: &str) -> Option<Hsla> {
    let hex = s.strip_prefix('#')?;
    if !matches!(hex.len(), 3 | 6 | 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    gpui::Rgba::try_from(s).ok().map(Into::into)
}

fn from_value(t: &Value, dark: bool) -> anyhow::Result<Theme> {
    let name = t["name"].as_str().unwrap_or("a theme");
    let colors = &t["colors"];
    let color = |key: &str| -> anyhow::Result<Hsla> {
        let Some(text) = colors[key].as_str() else {
            bail!("{name}: {key} is missing");
        };
        parse_color(text).ok_or_else(|| anyhow!("{name}: {key} = {text:?} is not a #hex color"))
    };
    for (key, value) in colors.as_object().into_iter().flatten() {
        let text = value.as_str().unwrap_or_default();
        if parse_color(text).is_none() {
            bail!("{name}: {key} = {text:?} is not a #hex color");
        }
    }
    let number = |key: &str| t[key].as_f64().map(|n| px(n as f32));
    let (background, danger) = (color("background")?, color("danger.background")?);
    // A selection stays see-through enough to read the text under it.
    let selection = color("selection.background")?;
    let selection = selection.alpha(selection.a.min(0.3));
    let highlight = match t.get("highlight") {
        Some(style) => serde_json::from_value::<HighlightThemeStyle>(style.clone())
            .with_context(|| format!("{name}: highlight"))?,
        None => default_highlight(dark).style.clone(),
    };
    let mut highlight_theme = (*default_highlight(dark)).clone();
    highlight_theme.name = name.to_string();
    highlight_theme.style = highlight;
    Ok(Theme {
        mode: if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        background,
        foreground: color("foreground")?,
        border: color("border")?,
        muted: color("muted.background")?,
        muted_foreground: color("muted.foreground")?,
        accent: color("accent.background")?,
        accent_foreground: color("accent.foreground")?,
        primary: color("primary.background")?,
        primary_foreground: color("primary.foreground")?,
        primary_hover: color("primary.hover.background")?,
        primary_active: color("primary.active.background")?,
        secondary: color("secondary.background")?,
        secondary_foreground: color("secondary.foreground")?,
        secondary_hover: color("secondary.hover.background")?,
        secondary_active: color("secondary.active.background")?,
        danger,
        danger_foreground: color("danger.foreground")?,
        danger_hover: background.blend(danger.opacity(0.9)),
        danger_active: darken(danger, if dark { 0.2 } else { 0.1 }),
        warning: color("warning.background")?,
        success: color("success.background")?,
        info: color("info.background")?,
        input: color("input.border")?,
        popover: color("popover.background")?,
        popover_foreground: color("popover.foreground")?,
        ring: color("ring")?,
        selection,
        overlay: color("overlay")?,
        link: color("link")?,
        tab_bar: color("tab_bar.background")?,
        sidebar: color("sidebar.background")?,
        sidebar_foreground: color("sidebar.foreground")?,
        sidebar_accent: color("sidebar.accent.background")?,
        sidebar_accent_foreground: color("sidebar.accent.foreground")?,
        sidebar_border: color("sidebar.border")?,
        red: color("base.red")?,
        green: color("base.green")?,
        blue: color("base.blue")?,
        yellow: color("base.yellow")?,
        magenta: color("base.magenta")?,
        cyan: color("base.cyan")?,
        chart_1: color("chart.1")?,
        chart_2: color("chart.2")?,
        chart_3: color("chart.3")?,
        chart_4: color("chart.4")?,
        chart_5: color("chart.5")?,
        font_family: SYSTEM_UI_FONT.into(),
        font_size: number("font.size").unwrap_or(px(16.)),
        mono_font_family: MONO.0.into(),
        mono_font_size: number("mono_font.size").unwrap_or(px(13.)),
        radius_lg: number("radius.lg").unwrap_or(px(8.)),
        highlight_theme: Arc::new(highlight_theme),
    })
}

pub(crate) fn default_highlight(dark: bool) -> Arc<HighlightTheme> {
    if dark {
        HighlightTheme::default_dark()
    } else {
        HighlightTheme::default_light()
    }
}

pub(crate) fn lighten(c: Hsla, by: f32) -> Hsla {
    Hsla {
        l: c.l * (1.0 + by.clamp(0., 1.)),
        ..c
    }
}

pub(crate) fn darken(c: Hsla, by: f32) -> Hsla {
    Hsla {
        l: c.l * (1.0 - by.clamp(0., 1.)),
        ..c
    }
}

/// `#RRGGBB`, or `#RRGGBBAA` when not opaque.
pub(crate) fn to_hex(c: Hsla) -> String {
    let rgb = c.to_rgb();
    let byte = |v: f32| (v * 255.) as u32;
    if rgb.a < 1. {
        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            byte(rgb.r),
            byte(rgb.g),
            byte(rgb.b),
            byte(c.a)
        )
    } else {
        format!("#{:02X}{:02X}{:02X}", byte(rgb.r), byte(rgb.g), byte(rgb.b))
    }
}

/// `default` when installed, else the first installed of `alternates`, else the system UI font.
pub fn first_installed(default: &str, alternates: &[&str], installed: &[String]) -> SharedString {
    std::iter::once(default)
        .chain(alternates.iter().copied())
        .find(|f| installed.iter().any(|n| n == f))
        .unwrap_or(SYSTEM_UI_FONT)
        .to_string()
        .into()
}

/// Names the families GPUI really draws with: the system UI font's installed stand-in (Linux), and an installed
/// monospace family. A missing lookup is paid on every text run, and a missing mono family can't lay out.
pub(super) fn installed_fonts(theme: &mut Theme, cx: &App) {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    let installed = NAMES.get_or_init(|| cx.text_system().all_font_names());
    theme.mono_font_family = first_installed(MONO.0, MONO.1, installed);
    if installed.is_empty() {
        return;
    }
    let text = cx.text_system();
    let resolved = text
        .get_font_for_id(text.resolve_font(&font(SYSTEM_UI_FONT)))
        .map(|f| f.family);
    if let Some(family) = resolved
        .filter(|f| f.as_ref() != SYSTEM_UI_FONT && installed.iter().any(|n| n == f.as_ref()))
    {
        theme.font_family = family;
    }
}
