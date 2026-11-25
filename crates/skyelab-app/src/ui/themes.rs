//! Settings → Appearance: the theme families (data in `themes/*.json`, gpui-component's theme format), accent
//! presets, density and text sizes, applied live to every window and saved under `[appearance]` in settings.toml.

use gpui::{App, AppContext as _, Global, Hsla, Task};
use gpui_component::{
    Colorize as _, Theme, ThemeConfig, ThemeMode, ThemeRegistry, try_parse_color,
};
use skyelab_core::{AppSettings, AppearanceSettings, ColorMode};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

/// A theme family: a light and a dark theme named `<label> Light` / `<label> Dark` in its file.
pub struct Family {
    pub key: &'static str,
    pub label: &'static str,
    pub about: &'static str,
    json: &'static str,
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

/// Registers every family's themes. Call once, after `gpui_component::init`.
pub(super) fn register(cx: &mut App) -> anyhow::Result<()> {
    let registry = ThemeRegistry::global_mut(cx);
    for f in &FAMILIES {
        registry.load_themes_from_str(f.json)?;
    }
    for f in &FAMILIES {
        for mode in ["Light", "Dark"] {
            let name = format!("{} {mode}", f.label);
            anyhow::ensure!(
                registry.themes().contains_key(name.as_str()),
                "theme {name} is missing"
            );
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

/// Puts the chosen family, accent and sizes into the light and dark themes and shows the chosen mode.
pub fn apply(cx: &mut App) {
    let s = get(cx).clamped();
    let f = family(&s.theme);
    let registry = ThemeRegistry::global(cx);
    let pick = |m: &str| {
        registry
            .themes()
            .get(format!("{} {m}", f.label).as_str())
            .cloned()
    };
    let (Some(light), Some(dark)) = (pick("Light"), pick("Dark")) else {
        return; // not registered (a test without `install_theme`): keep gpui-component's themes
    };
    let (light, dark) = (adjust(&light, &s, false), adjust(&dark, &s, true));
    COMPACT.store(s.compact, Ordering::Relaxed);
    // Every family has them (`terminal_colors_test`); a broken file keeps the last family's.
    if let Some(colors) = ansi(f) {
        cx.set_global(colors);
    }
    let theme = Theme::global_mut(cx);
    theme.light_theme = Rc::new(light);
    theme.dark_theme = Rc::new(dark);
    let system = cx.window_appearance().into();
    Theme::change(mode(&s, system), None, cx);
}

/// Follows the setting, or the system appearance when it says System. Called when a window opens and whenever its
/// appearance changes.
pub fn sync(window: &mut gpui::Window, cx: &mut App) {
    let m = mode(&get(cx), window.appearance().into());
    if Theme::global(cx).mode != m {
        Theme::change(m, Some(window), cx);
    }
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
        let colors: Vec<Hsla> = list.iter().filter_map(|c| parse_hex(c.as_str()?)).collect();
        colors.try_into().ok()
    };
    Some(skyelab_terminal::AnsiColors {
        light: theme("light")?,
        dark: theme("dark")?,
    })
}

pub(crate) fn parse_hex(s: &str) -> Option<Hsla> {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).ok()?;
    Some(gpui::rgb(v).into())
}

/// `base` with the text sizes and, when set, the accent preset written into it.
pub(crate) fn adjust(base: &ThemeConfig, s: &AppearanceSettings, dark: bool) -> ThemeConfig {
    let mut c = base.clone();
    // A theme with no `highlight` keeps whatever code colors were applied before (gpui-component starts with its
    // light ones), so a dark theme drew code in light-theme colors. Give each mode its own default.
    if c.highlight.is_none() {
        let default = if dark {
            gpui_component::highlighter::HighlightTheme::default_dark()
        } else {
            gpui_component::highlighter::HighlightTheme::default_light()
        };
        c.highlight = Some(default.style.clone());
    }
    c.font_size = Some(f32::from(s.ui_font_size));
    c.mono_font_size = Some(f32::from(s.code_font_size));
    if let Some(a) = accent(s.accent.as_deref()) {
        let (fill, text) = if dark { a.dark } else { a.light };
        let Some(color) = parse_hex(fill) else {
            return c;
        };
        let shade = |up: bool| {
            let shaded = if up == dark {
                color.lighten(0.08)
            } else {
                color.darken(0.08)
            };
            Some(shaded.to_hex().into())
        };
        let k = &mut c.colors;
        k.primary = Some(fill.into());
        k.primary_hover = shade(true);
        k.primary_active = shade(false);
        k.primary_foreground = Some(text.into());
        k.sidebar_primary = Some(fill.into());
        k.sidebar_primary_foreground = Some(text.into());
        // A preset is tuned for neutral canvases; a tinted one (Nord's slate, Gruvbox's cream) can sink it.
        let surfaces: Vec<Hsla> = [&k.background, &k.sidebar, &k.popover]
            .into_iter()
            .filter_map(|c| try_parse_color(c.as_deref()?).ok())
            .collect();
        let link = parse_hex(if dark { a.dark_link } else { fill }).unwrap_or(color);
        k.ring = Some(readable(color, &surfaces, 3.0, dark).to_hex().into());
        k.link = Some(readable(link, &surfaces, 4.5, dark).to_hex().into());
        k.list_active = Some(format!("{fill}{}", if dark { "26" } else { "1a" }).into());
        k.list_active_border = Some(fill.into());
        k.selection = Some(format!("{fill}{}", if dark { "4d" } else { "33" }).into());
    }
    c
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

/// A family's canvas, text and accent in light or dark, resolved the way the live theme resolves them (`None` before
/// [`super::install_theme`]): the onboarding's theme tiles paint each family with it.
pub fn swatch(key: &str, dark: bool, cx: &App) -> Option<[Hsla; 3]> {
    let f = FAMILIES.iter().find(|f| f.key == key)?;
    let name = format!("{} {}", f.label, if dark { "Dark" } else { "Light" });
    let config = ThemeRegistry::global(cx)
        .themes()
        .get(name.as_str())?
        .clone();
    // The user's accent preset too, as `apply` bakes it in, so the current family's tile matches the live theme.
    let config = Rc::new(adjust(&config, &get(cx).clamped(), dark));
    let mut theme = Theme::global(cx).clone();
    theme.apply_config(&config);
    Some([theme.background, theme.foreground, theme.primary])
}
