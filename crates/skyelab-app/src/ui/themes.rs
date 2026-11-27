//! Settings → Appearance: the theme families (data in `themes/*.json`), accent presets, density and text sizes,
//! applied live to every window and saved under `[appearance]` in settings.toml.

use super::theme::{Theme, ThemeMode};
use super::theme_load::{self, darken, lighten, parse_color, to_hex};
use gpui::{App, AppContext as _, Global, Hsla, Task};
use serde_json::Value;
use skyelab_core::{AppSettings, AppearanceSettings, ColorMode};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

/// A theme family: a light and a dark theme named `<label> Light` / `<label> Dark` in its file.
pub struct Family {
    pub key: &'static str,
    pub label: &'static str,
    pub about: &'static str,
    pub(crate) json: &'static str,
}

pub const FAMILIES: [Family; 14] = [
    Family {
        key: "skyelab",
        label: "SkyeLab",
        about: "Porcelain by day, midnight slate by night.",
        json: include_str!("../../themes/skyelab.json"),
    },
    Family {
        key: "classic",
        label: "Classic",
        about: "The first SkyeLab: zinc and true black.",
        json: include_str!("../../themes/classic.json"),
    },
    Family {
        key: "skye",
        label: "Skye",
        about: "Midnight navy from the app icon.",
        json: include_str!("../../themes/skye.json"),
    },
    Family {
        key: "graphite",
        label: "Graphite",
        about: "Soft warm grays and teal.",
        json: include_str!("../../themes/graphite.json"),
    },
    Family {
        key: "dawn",
        label: "Dawn",
        about: "Warm paper by day, embers by night.",
        json: include_str!("../../themes/dawn.json"),
    },
    Family {
        key: "grove",
        label: "Grove",
        about: "Quiet greens for long sessions.",
        json: include_str!("../../themes/grove.json"),
    },
    Family {
        key: "catppuccin",
        label: "Catppuccin",
        about: "Soft pastels: Latte by day, Mocha by night.",
        json: include_str!("../../themes/catppuccin.json"),
    },
    Family {
        key: "rose-pine",
        label: "Rosé Pine",
        about: "Muted rose and pine on dusky violet.",
        json: include_str!("../../themes/rose-pine.json"),
    },
    Family {
        key: "tokyo-night",
        label: "Tokyo Night",
        about: "Neon blues over a deep night sky.",
        json: include_str!("../../themes/tokyo-night.json"),
    },
    Family {
        key: "kanagawa",
        label: "Kanagawa",
        about: "Ink and woodblock tones after Hokusai.",
        json: include_str!("../../themes/kanagawa.json"),
    },
    Family {
        key: "everforest",
        label: "Everforest",
        about: "Warm, low-glare forest greens.",
        json: include_str!("../../themes/everforest.json"),
    },
    Family {
        key: "gruvbox",
        label: "Gruvbox",
        about: "Retro warm browns with bright accents.",
        json: include_str!("../../themes/gruvbox.json"),
    },
    Family {
        key: "nord",
        label: "Nord",
        about: "Arctic frost blues on polar night.",
        json: include_str!("../../themes/nord.json"),
    },
    Family {
        key: "flexoki",
        label: "Flexoki",
        about: "Ink on paper, tuned for reading.",
        json: include_str!("../../themes/flexoki.json"),
    },
];

/// An accent preset: its fill and the text on it, for light and dark themes.
pub struct Accent {
    pub key: &'static str,
    pub label: &'static str,
    pub light: (&'static str, &'static str),
    pub dark: (&'static str, &'static str),
    /// Links on a dark background: the dark fill is too deep for 4.5:1 text in blue and violet.
    pub dark_link: &'static str,
}

pub const ACCENTS: [Accent; 6] = [
    Accent {
        key: "blue",
        label: "Blue",
        light: ("#1b4ed8", "#ffffff"),
        dark: ("#346bf1", "#ffffff"),
        dark_link: "#7aa2f7",
    },
    Accent {
        key: "violet",
        label: "Violet",
        light: ("#6d28d9", "#ffffff"),
        dark: ("#7c3aed", "#ffffff"),
        dark_link: "#a78bfa",
    },
    Accent {
        key: "teal",
        label: "Teal",
        light: ("#0f766e", "#ffffff"),
        dark: ("#2dd4bf", "#042f2e"),
        dark_link: "#2dd4bf",
    },
    Accent {
        key: "green",
        label: "Green",
        light: ("#15803d", "#ffffff"),
        dark: ("#4ade80", "#052e16"),
        dark_link: "#4ade80",
    },
    Accent {
        key: "amber",
        label: "Amber",
        light: ("#b45309", "#ffffff"),
        dark: ("#fbbf24", "#451a03"),
        dark_link: "#fbbf24",
    },
    Accent {
        key: "rose",
        label: "Rose",
        light: ("#be123c", "#ffffff"),
        dark: ("#fb7185", "#4c0519"),
        dark_link: "#fb7185",
    },
];

/// The family for `key`, else the default.
pub fn family(key: &str) -> &'static Family {
    FAMILIES
        .iter()
        .find(|f| f.key == key)
        .unwrap_or(&FAMILIES[0])
}

pub fn accent(key: Option<&str>) -> Option<&'static Accent> {
    key.and_then(|k| ACCENTS.iter().find(|a| a.key == k))
}

// shortcut: a process-wide flag so `ui::row` (which only gets `&Theme`) can read it; a theme change refreshes every
// window, so a switch shows at once. Upgrade to a field on a SkyeLab theme wrapper if more knobs need it.
static COMPACT: AtomicBool = AtomicBool::new(false);

/// Compact density: tighter rows and cards.
pub fn compact() -> bool {
    COMPACT.load(Ordering::Relaxed)
}

/// The appearance in use and the file it saves to.
#[derive(Default)]
pub struct Appearance {
    path: Option<PathBuf>,
    pub settings: AppearanceSettings,
}

impl Global for Appearance {}

/// Checks that every family file loads in both modes. Call once, before the first [`apply`].
pub(super) fn register() -> anyhow::Result<()> {
    for f in &FAMILIES {
        for dark in [false, true] {
            theme_load::resolve(f.json, dark, &AppearanceSettings::default())
                .map_err(|e| e.context(format!("theme family {}", f.key)))?;
        }
    }
    Ok(())
}

/// Reads `[appearance]` from settings.toml (missing or unreadable: the defaults) and applies it.
pub fn load(path: Option<PathBuf>, cx: &mut App) {
    let settings = path
        .as_deref()
        .and_then(|p| AppSettings::load(p).ok())
        .map(|s| s.appearance.clamped())
        .unwrap_or_default();
    cx.set_global(Appearance { path, settings });
    apply(cx);
}

/// The appearance now (the defaults when none is loaded, as in tests).
pub fn get(cx: &App) -> AppearanceSettings {
    cx.try_global::<Appearance>()
        .map(|a| a.settings.clone())
        .unwrap_or_default()
}

/// Uses `settings` in every window now and saves them into the file as it is on disk (other settings untouched).
pub fn set(settings: AppearanceSettings, cx: &mut App) -> Task<anyhow::Result<()>> {
    let settings = settings.clamped();
    let path = cx.try_global::<Appearance>().and_then(|a| a.path.clone());
    cx.set_global(Appearance {
        path: path.clone(),
        settings: settings.clone(),
    });
    apply(cx);
    let n = next_save();
    cx.background_spawn(async move {
        match path {
            Some(p) => save(n, &p, settings),
            None => Ok(()),
        }
    })
}

/// Numbers each save in the order `set` was called.
pub(crate) fn next_save() -> u64 {
    static SAVES: AtomicU64 = AtomicU64::new(0);
    SAVES.fetch_add(1, Ordering::SeqCst) + 1
}

/// Writes save `n` unless a newer one already landed: quick changes save on the thread pool in any order.
pub(crate) fn save(n: u64, path: &Path, settings: AppearanceSettings) -> anyhow::Result<()> {
    static WRITTEN: Mutex<BTreeMap<PathBuf, u64>> = Mutex::new(BTreeMap::new());
    let mut written = WRITTEN.lock().unwrap_or_else(PoisonError::into_inner);
    if written.get(path).is_some_and(|w| *w > n) {
        return Ok(());
    }
    AppSettings::update(path, |s| s.appearance = settings)?;
    written.insert(path.to_owned(), n);
    Ok(())
}

/// The mode to show: the verify override (debug builds), else the setting, else the system's.
fn mode(s: &AppearanceSettings, system: ThemeMode) -> ThemeMode {
    match std::env::var("SKYELAB_VERIFY_THEME").as_deref() {
        Ok("dark") if cfg!(debug_assertions) => return ThemeMode::Dark,
        Ok("light") if cfg!(debug_assertions) => return ThemeMode::Light,
        _ => {}
    }
    match s.mode {
        ColorMode::System => system,
        ColorMode::Light => ThemeMode::Light,
        ColorMode::Dark => ThemeMode::Dark,
    }
}

/// Puts the chosen family, accent and sizes into our theme and gpui-component's, in the mode to show.
pub fn apply(cx: &mut App) {
    let system = cx.window_appearance().into();
    show(mode(&get(cx), system), cx);
}

/// Follows the setting, or the system appearance when it says System. Called when a window opens and whenever its
/// appearance changes.
pub fn sync(window: &mut gpui::Window, cx: &mut App) {
    follow(window.appearance(), window, cx);
}

/// Shows the mode for `appearance` (the window's): what [`sync`] does with the window's own.
pub fn follow(appearance: gpui::WindowAppearance, window: &mut gpui::Window, cx: &mut App) {
    let m = mode(&get(cx), appearance.into());
    if cx.try_global::<Theme>().is_none_or(|t| t.mode != m) {
        show(m, cx);
        window.refresh();
    }
}

fn show(m: ThemeMode, cx: &mut App) {
    let s = get(cx).clamped();
    let f = family(&s.theme);
    let dark = m.is_dark();
    // `register` checked every file at start.
    let mut theme = match theme_load::resolve(f.json, dark, &s) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("theme {}: {e:#}", f.key);
            return;
        }
    };
    theme_load::installed_fonts(&mut theme, cx);
    COMPACT.store(s.compact, Ordering::Relaxed);
    super::motion::apply_reduce_motion(cx);
    // Every family has them (`terminal_colors_test`); a broken file keeps the last family's.
    if let Some(colors) = ansi(f) {
        cx.set_global(colors);
    }
    cx.set_global(theme);
    super::theme_bridge::write(
        [adjusted(f.key, &s, false), adjusted(f.key, &s, true)],
        dark,
        cx,
    );
    cx.refresh_windows();
}

/// The family's terminal colors: each theme's `terminal.ansi`, 16 hex colors (gpui-component ignores the key).
pub(crate) fn ansi(f: &Family) -> Option<skyelab_terminal::AnsiColors> {
    let file: serde_json::Value = serde_json::from_str(f.json).ok()?;
    let theme = |mode: &str| -> Option<[Hsla; 16]> {
        let t = file["themes"]
            .as_array()?
            .iter()
            .find(|t| t["mode"] == mode)?;
        let list = t["terminal.ansi"].as_array()?;
        let colors: Vec<Hsla> = list
            .iter()
            .filter_map(|c| parse_color(c.as_str()?))
            .collect();
        colors.try_into().ok()
    };
    Some(skyelab_terminal::AnsiColors {
        light: theme("light")?,
        dark: theme("dark")?,
    })
}

/// One theme of family `key` as JSON, with the accent preset and text sizes of `s` written in.
pub fn adjusted(key: &str, s: &AppearanceSettings, dark: bool) -> Value {
    let file: Value = serde_json::from_str(family(key).json).unwrap_or_default();
    let mut theme = theme_load::pick(&file, dark).cloned().unwrap_or_default();
    adjust(&mut theme, s, dark);
    theme
}

/// Writes the text sizes, the mode's code colors and, when set, the accent preset into one theme of a family file.
pub(crate) fn adjust(theme: &mut Value, s: &AppearanceSettings, dark: bool) {
    // A theme with no `highlight` would keep whatever code colors were applied before, so a dark theme drew code in
    // light-theme colors. Give each mode its own default.
    if theme.get("highlight").is_none()
        && let Ok(style) = serde_json::to_value(&theme_load::default_highlight(dark).style)
    {
        theme["highlight"] = style;
    }
    theme["font.size"] = f32::from(s.ui_font_size).into();
    theme["mono_font.size"] = f32::from(s.code_font_size).into();
    let Some(a) = accent(s.accent.as_deref()) else {
        return;
    };
    let (fill, text) = if dark { a.dark } else { a.light };
    let Some(color) = parse_color(fill) else {
        return;
    };
    let shade = |up: bool| {
        let shaded = if up == dark {
            lighten(color, 0.08)
        } else {
            darken(color, 0.08)
        };
        to_hex(shaded)
    };
    let k = &mut theme["colors"];
    // A preset is tuned for neutral canvases; a tinted one (Nord's slate, Gruvbox's cream) can sink it.
    let surfaces: Vec<Hsla> = ["background", "sidebar.background", "popover.background"]
        .into_iter()
        .filter_map(|key| parse_color(k[key].as_str()?))
        .collect();
    let link = parse_color(if dark { a.dark_link } else { fill }).unwrap_or(color);
    let alpha =
        |light: &str, on_dark: &str| format!("{fill}{}", if dark { on_dark } else { light });
    for (key, value) in [
        ("primary.background", fill.to_string()),
        ("primary.hover.background", shade(true)),
        ("primary.active.background", shade(false)),
        ("primary.foreground", text.to_string()),
        ("sidebar.primary.background", fill.to_string()),
        ("sidebar.primary.foreground", text.to_string()),
        ("ring", to_hex(readable(color, &surfaces, 3.0, dark))),
        ("link", to_hex(readable(link, &surfaces, 4.5, dark))),
        ("list.active.background", alpha("1a", "26")),
        ("list.active.border", fill.to_string()),
        ("selection.background", alpha("33", "4d")),
    ] {
        k[key] = value.into();
    }
}

/// WCAG relative luminance.
fn luminance(c: Hsla) -> f32 {
    let c = gpui::Rgba::from(c);
    let f = |v: f32| {
        if v <= 0.039_28 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
}

/// WCAG contrast ratio between two opaque colors.
pub(crate) fn ratio(a: Hsla, b: Hsla) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// `c`, lightened (dark themes) or darkened (light) until it reaches `min`:1 on every surface.
fn readable(mut c: Hsla, on: &[Hsla], min: f32, dark: bool) -> Hsla {
    for _ in 0..50 {
        if on.iter().all(|&b| ratio(c, b) >= min) {
            break;
        }
        c.l = (c.l + if dark { 0.02 } else { -0.02 }).clamp(0., 1.);
    }
    c
}

/// A family's canvas, text and accent in light or dark, resolved the way the live theme resolves them, the user's
/// accent preset included: the onboarding's theme tiles paint each family with it.
pub fn swatch(key: &str, dark: bool, cx: &App) -> Option<[Hsla; 3]> {
    let f = FAMILIES.iter().find(|f| f.key == key)?;
    let t = theme_load::resolve(f.json, dark, &get(cx).clamped()).ok()?;
    Some([t.background, t.foreground, t.primary])
}
