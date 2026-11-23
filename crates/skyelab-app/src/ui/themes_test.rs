//! Theme families load, and switching one changes the live theme, fonts and density.

use super::themes::{self, ACCENTS, FAMILIES};
use gpui::{TestAppContext, px};
use gpui_component::{ActiveTheme as _, Theme, ThemeMode};
use skyelab_core::{AppearanceSettings, ColorMode};

fn setup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        super::install_theme(cx).expect("every theme file parses and names both modes");
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
