//! Theme families load, and switching one changes the live theme, fonts and density.

use super::themes::{self, ACCENTS, FAMILIES, ratio};
use gpui::{TestAppContext, px};
use gpui_component::{ActiveTheme as _, Theme, ThemeMode};
use skyelab_core::{AppearanceSettings, ColorMode};

fn setup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        super::install_theme(cx).expect("every theme file parses and names both modes");
    });
}

/// Onboarding's theme tiles paint each family from its swatch: a light canvas in Light, a dark one in Dark.
#[gpui::test]
fn every_family_has_a_light_and_a_dark_swatch(cx: &mut TestAppContext) {
    setup(cx);
    cx.update(|cx| {
        for f in &FAMILIES {
            let light =
                themes::swatch(f.key, false, cx).unwrap_or_else(|| panic!("{} light", f.key));
            let dark = themes::swatch(f.key, true, cx).unwrap_or_else(|| panic!("{} dark", f.key));
            // Light: a bright canvas under darker text; dark: the reverse. Readable either way.
            let (lc, lt) = (light[0].to_rgb(), light[1].to_rgb());
            let (dc, dt) = (dark[0].to_rgb(), dark[1].to_rgb());
            assert!(
                lc.r + lc.g + lc.b > 2.2 && lc.r + lc.g + lc.b > lt.r + lt.g + lt.b,
                "{} light: {light:?}",
                f.key
            );
            assert!(
                dc.r + dc.g + dc.b < 0.8 && dc.r + dc.g + dc.b < dt.r + dt.g + dt.b,
                "{} dark: {dark:?}",
                f.key
            );
            assert!(ratio(light[1], light[0]) >= 4.5, "{} light text", f.key);
            assert!(ratio(dark[1], dark[0]) >= 4.5, "{} dark text", f.key);
        }
    });
}

#[gpui::test]
fn every_family_registers_light_and_dark(cx: &mut TestAppContext) {
    setup(cx);
    cx.update(|cx| {
        let registry = gpui_component::ThemeRegistry::global(cx);
        for f in &FAMILIES {
            for (mode, dark) in [("Light", false), ("Dark", true)] {
                let t = registry
                    .themes()
                    .get(format!("{} {mode}", f.label).as_str());
                let t = t.unwrap_or_else(|| panic!("{} {mode} missing", f.label));
                assert_eq!(t.mode.is_dark(), dark, "{} {mode}", f.label);
            }
        }
    });
}

#[gpui::test]
fn switching_applies_family_mode_accent_sizes_and_density(cx: &mut TestAppContext) {
    setup(cx);
    let before = cx.update(|cx| {
        Theme::change(ThemeMode::Dark, None, cx);
        cx.theme().background
    });
    cx.update(|cx| {
        let s = AppearanceSettings {
            mode: ColorMode::Dark,
            theme: "dawn".into(),
            accent: Some("rose".into()),
            compact: true,
            ui_font_size: 16,
            code_font_size: 15,
            ..Default::default()
        };
        themes::set(s, cx).detach();
    });
    cx.update(|cx| {
        let t = cx.theme();
        assert!(t.is_dark());
        assert_eq!(t.dark_theme.name, "Dawn Dark");
        assert_ne!(t.background, before, "the canvas changed with the family");
        assert_eq!(t.font_size, px(16.));
        assert_eq!(t.mono_font_size, px(15.));
        let rose = gpui::Hsla::from(gpui::rgb(0xfb7185));
        assert!(
            (t.primary.h - rose.h).abs() < 0.01,
            "accent applied: {:?}",
            t.primary
        );
        assert!(themes::get(cx).compact, "saved as compact");
    });
    // Light mode, defaults back.
    cx.update(|cx| {
        let s = AppearanceSettings {
            mode: ColorMode::Light,
            ..Default::default()
        };
        themes::set(s, cx).detach();
        assert!(!cx.theme().is_dark());
        assert_eq!(cx.theme().light_theme.name, "SkyeLab Light");
        assert!(!themes::get(cx).compact);
    });
    assert_eq!(ACCENTS.len(), 6);
}

#[gpui::test]
fn code_is_highlighted_for_the_shown_mode(cx: &mut TestAppContext) {
    setup(cx);
    for (mode, dark) in [
        (ColorMode::Dark, true),
        (ColorMode::Light, false),
        (ColorMode::Dark, true),
    ] {
        cx.update(|cx| {
            let s = AppearanceSettings {
                mode,
                ..Default::default()
            };
            themes::set(s, cx).detach();
            // Dark code colors on a dark canvas: `fn main` drew in navy on black.
            assert_eq!(
                cx.theme().highlight_theme.appearance.is_dark(),
                dark,
                "{mode:?}"
            );
        });
    }
}

#[gpui::test]
fn the_choice_is_read_from_and_saved_to_settings_toml(cx: &mut TestAppContext) {
    setup(cx);
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("settings.toml");
    std::fs::write(
        &path,
        "[appearance]\ntheme = \"grove\"\nui_font_size = 40\n",
    )
    .expect("write");
    cx.update(|cx| themes::load(Some(path.clone()), cx));
    cx.update(|cx| {
        assert_eq!(
            cx.theme().light_theme.name,
            "Grove Light",
            "loaded at start"
        );
        assert_eq!(
            themes::get(cx).ui_font_size,
            18,
            "a hand-edited size is clamped on load"
        );
    });
    cx.update(|cx| {
        let s = AppearanceSettings {
            accent: Some("violet".into()),
            ..themes::get(cx)
        };
        themes::set(s, cx).detach();
    });
    cx.run_until_parked();
    let saved = skyelab_core::AppSettings::load(&path)
        .expect("load")
        .appearance;
    assert_eq!(
        (saved.theme.as_str(), saved.accent.as_deref()),
        ("grove", Some("violet"))
    );
}

#[test]
fn an_older_save_finishing_last_never_wins() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("settings.toml");
    let (older, newer) = (themes::next_save(), themes::next_save());
    let pick = |accent: &str| AppearanceSettings {
        accent: Some(accent.into()),
        ..Default::default()
    };
    // Two quick clicks: the newer save's thread runs first.
    themes::save(newer, &path, pick("rose")).expect("save");
    themes::save(older, &path, pick("violet")).expect("save");
    let saved = skyelab_core::AppSettings::load(&path).expect("load");
    assert_eq!(saved.appearance.accent.as_deref(), Some("rose"));
}

/// gpui-component's `apply_config` drops a color it can't parse for its default (`unwrap_or`), so a typo or a color
/// name it doesn't know (0.11 has no `zinc`) silently paints the wrong color. Parse each one the way it does.
#[test]
fn every_theme_color_parses() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("themes");
    let mut bad = Vec::new();
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).expect("themes dir") {
        let path = entry.expect("theme entry").path();
        let file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read theme"))
                .expect("theme json");
        for theme in file["themes"].as_array().expect("themes list") {
            let colors = theme["colors"].as_object().expect("colors");
            for (key, value) in colors {
                seen += 1;
                let value = value.as_str().expect("color is a string");
                if let Err(e) = gpui_component::try_parse_color(value) {
                    bad.push(format!(
                        "{} {}: {key} = {value} ({e})",
                        path.display(),
                        theme["name"]
                    ));
                }
            }
        }
    }
    assert!(seen > 100, "read only {seen} colors from {}", dir.display());
    assert!(
        bad.is_empty(),
        "colors gpui-component can't parse:\n{}",
        bad.join("\n")
    );
}
