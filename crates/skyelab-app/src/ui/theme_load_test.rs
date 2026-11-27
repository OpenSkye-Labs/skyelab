//! Own UI kit, stage 2a (.agent/plans/2026-11-27-own-ui-kit-theme.md), criterion 2: our loader reads a family's JSON by
//! itself (no `App`, no gpui-component), and refuses a color it can't read instead of painting a default.

use super::theme_load::resolve;
use super::themes::FAMILIES;
use gpui::px;
use skyelab_core::AppearanceSettings;

fn file_theme(json: &serde_json::Value, dark: bool) -> &serde_json::Value {
    let mode = if dark { "dark" } else { "light" };
    json["themes"]
        .as_array()
        .expect("themes list")
        .iter()
        .find(|t| t["mode"] == mode)
        .expect("theme for the mode")
}

fn file_color(json: &serde_json::Value, dark: bool, key: &str) -> gpui::Hsla {
    let hex = file_theme(json, dark)["colors"][key]
        .as_str()
        .expect("a color string");
    gpui::Rgba::try_from(hex)
        .unwrap_or_else(|_| panic!("{key} = {hex} is hex"))
        .into()
}

#[test]
fn the_loader_reads_each_family_file_by_itself() {
    let s = AppearanceSettings {
        ui_font_size: 15,
        code_font_size: 12,
        ..Default::default()
    };
    for f in &FAMILIES {
        let json: serde_json::Value = serde_json::from_str(f.json).expect("theme json");
        for dark in [false, true] {
            let t = resolve(f.json, dark, &s).unwrap_or_else(|e| panic!("{} {dark}: {e:#}", f.key));
            let what = format!("{} dark={dark}", f.key);
            assert_eq!(t.is_dark(), dark, "{what}");
            for (key, got) in [
                ("background", t.background),
                ("foreground", t.foreground),
                ("primary.background", t.primary),
                ("border", t.border),
                ("sidebar.background", t.sidebar),
                ("danger.background", t.danger),
                ("tab_bar.background", t.tab_bar),
                ("input.border", t.input),
                ("base.red", t.red),
            ] {
                assert_eq!(got, file_color(&json, dark, key), "{what} {key}");
            }
            assert_eq!(
                (t.font_size, t.mono_font_size),
                (px(15.), px(12.)),
                "{what} sizes"
            );
            let radius = file_theme(&json, dark)["radius.lg"]
                .as_f64()
                .expect("radius.lg");
            assert_eq!(t.radius_lg, px(radius as f32), "{what} radius.lg");
        }
    }
}

/// Garbage, and a named color gpui-component would accept (`red-500`): ours reads hex only.
#[test]
fn a_color_it_cannot_read_is_an_error_naming_the_key() {
    let f = &FAMILIES[0];
    for bad in ["zinc-#", "red-500\", \"x\": \"#"] {
        let broken = f
            .json
            .replacen("\"border\": \"#", &format!("\"border\": \"{bad}"), 1);
        assert_ne!(broken, f.json, "the fixture changed the border");
        let err = resolve(&broken, false, &AppearanceSettings::default())
            .err()
            .unwrap_or_else(|| panic!("a border of {bad:?} is refused"));
        let msg = format!("{err:#}");
        assert!(msg.contains("border"), "names the key: {msg}");
        assert!(msg.contains(f.label), "names the theme: {msg}");
    }
}
