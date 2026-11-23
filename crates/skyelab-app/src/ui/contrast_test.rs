//! WCAG contrast over every theme, mode and accent: body text needs 4.5:1.

use super::themes::{ACCENTS, FAMILIES, adjust};
use gpui::{Hsla, Rgba, TestAppContext};
use gpui_component::{Theme, ThemeMode};
use skyelab_core::AppearanceSettings;

fn luminance(c: Hsla) -> f32 {
    let c = Rgba::from(c);
    let f = |v: f32| {
        if v <= 0.039_28 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
}

pub(super) fn ratio(a: Hsla, b: Hsla) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[gpui::test]
fn every_theme_meets_body_text_contrast(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        super::install_theme(cx).expect("themes load");
        let mut low = Vec::new();
        let accents = std::iter::once(None).chain(ACCENTS.iter().map(|a| Some(a.key.to_owned())));
        for f in &FAMILIES {
            for accent in accents.clone() {
                for (mode, dark) in [("Light", false), ("Dark", true)] {
                    let name = format!("{} {mode}", f.label);
                    let base =
                        gpui_component::ThemeRegistry::global(cx).themes()[name.as_str()].clone();
                    let s = AppearanceSettings {
                        accent: accent.clone(),
                        ..Default::default()
                    };
                    let config = std::rc::Rc::new(adjust(&base, &s, dark));
                    let theme = Theme::global_mut(cx);
                    if dark {
                        theme.dark_theme = config;
                    } else {
                        theme.light_theme = config;
                    }
                    Theme::change(
                        if dark {
                            ThemeMode::Dark
                        } else {
                            ThemeMode::Light
                        },
                        None,
                        cx,
                    );
                    let t = Theme::global(cx);
                    let mut check = |what: &str, fg: Hsla, bg: Hsla| {
                        let r = ratio(fg, bg);
                        if r < 4.5 {
                            low.push(format!("{name} {accent:?} {what}: {r:.2}"));
                        }
                    };
                    for (surface, bg) in [
                        ("background", t.background),
                        ("sidebar", t.sidebar),
                        ("popover", t.popover),
                    ] {
                        assert_eq!(bg.a, 1.0, "{name} {surface} is opaque");
                        check(&format!("text on {surface}"), t.foreground, bg);
                        check(&format!("muted on {surface}"), t.muted_foreground, bg);
                    }
                    check("button text", t.primary_foreground, t.primary);
                    check("link on background", t.link, t.background);
                }
            }
        }
        assert!(low.is_empty(), "below 4.5:1:\n{}", low.join("\n"));
    });
}
