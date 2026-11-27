//! Every theme family carries 16 terminal ANSI colors for light and dark, readable on its terminal's background,
//! and the terminal uses them and the Appearance code font size.

use super::contrast_test::ratio;
use super::themes::{FAMILIES, ansi, apply, get, set};
use crate::ui::Theme;
use gpui::TestAppContext;

#[gpui::test]
fn every_theme_has_readable_terminal_colors(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        super::install_theme(cx).expect("themes load");
        let mut low = Vec::new();
        for f in &FAMILIES {
            let colors =
                ansi(f).unwrap_or_else(|| panic!("{} has terminal.ansi for both modes", f.label));
            for (mode, dark, list) in [("Light", false, colors.light), ("Dark", true, colors.dark)]
            {
                let name = format!("{} {mode}", f.label);
                let bg = super::theme_load::resolve(f.json, dark, &Default::default())
                    .expect("theme loads")
                    .background;
                // Black on a dark theme and white on a light one are the background's own side.
                let exempt: &[usize] = if dark { &[0] } else { &[7, 15] };
                for (i, c) in list.iter().enumerate() {
                    let r = ratio(*c, bg);
                    if !exempt.contains(&i) && r < 3.0 {
                        low.push(format!("{name} ansi {i}: {r:.2}"));
                    }
                }
            }
        }
        assert!(
            low.is_empty(),
            "below 3:1 on the terminal background:\n{}",
            low.join("\n")
        );
    });
}

#[gpui::test]
fn the_terminal_follows_the_family_and_the_code_font_size(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        super::install_theme(cx).expect("themes load");
        let grove = FAMILIES.iter().find(|f| f.key == "grove").expect("grove");
        let mut s = get(cx);
        s.theme = grove.key.into();
        s.code_font_size = 16;
        set(s, cx).detach();
        apply(cx);
        let want = ansi(grove).expect("grove's colors");
        let got = cx.global::<skyelab_terminal::AnsiColors>();
        assert_eq!(got.dark, want.dark);
        assert_eq!(got.light, want.light);
        crate::ui::themes::set(
            skyelab_core::AppearanceSettings {
                mode: skyelab_core::ColorMode::Dark,
                ..crate::ui::themes::get(cx)
            },
            cx,
        )
        .detach();
        assert_eq!(
            Theme::global(cx).mono_font_size,
            gpui::px(16.),
            "the terminal's text size"
        );
    });
}
