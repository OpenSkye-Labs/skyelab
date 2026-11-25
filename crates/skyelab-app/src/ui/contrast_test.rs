//! WCAG contrast over every theme, mode and accent: body text, muted text and links need 4.5:1 on every surface
//! (background, sidebar, cards); the focus ring, which marks rather than carries text, 3:1.

pub(super) use super::themes::ratio;
use super::themes::{ACCENTS, FAMILIES, adjust};
use gpui::{Hsla, Rgba, TestAppContext};
use gpui_component::{Theme, ThemeMode};
use skyelab_core::AppearanceSettings;

fn composite(fg: Hsla, bg: Hsla) -> Hsla {
    let f = Rgba::from(fg);
    let b = Rgba::from(bg);
    let a = f.a;
    Rgba {
        r: f.r * a + b.r * (1. - a),
        g: f.g * a + b.g * (1. - a),
        b: f.b * a + b.b * (1. - a),
        a: 1.,
    }
    .into()
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
                    let mut check_at = |min: f32, what: &str, fg: Hsla, bg: Hsla| {
                        let r = ratio(fg, bg);
                        if r < min {
                            low.push(format!("{name} {accent:?} {what}: {r:.2} < {min}"));
                        }
                    };
                    let mut check = |what: &str, fg: Hsla, bg: Hsla| check_at(4.5, what, fg, bg);
                    for (surface, bg) in [
                        ("background", t.background),
                        ("sidebar", t.sidebar),
                        ("popover", t.popover),
                    ] {
                        assert_eq!(bg.a, 1.0, "{name} {surface} is opaque");
                        check(&format!("text on {surface}"), t.foreground, bg);
                        check(&format!("muted on {surface}"), t.muted_foreground, bg);
                    }
                    for (surface, fill) in [
                        ("secondary", t.secondary),
                        ("muted", t.muted),
                        ("accent", t.accent),
                    ] {
                        let bg = composite(fill, t.background);
                        check(&format!("muted on {surface}"), t.muted_foreground, bg);
                    }
                    check("primary button text", t.primary_foreground, t.primary);
                    check("danger button text", t.danger_foreground, t.danger);
                    check("success button text", t.success_foreground, t.success);
                    check("warning button text", t.warning_foreground, t.warning);
                    check("info button text", t.info_foreground, t.info);
                    for (kind, color) in [
                        ("danger", t.danger),
                        ("success", t.success),
                        ("warning", t.warning),
                        ("info", t.info),
                    ] {
                        let ink = super::ink(t, color);
                        check(&format!("{kind} ink on background"), ink, t.background);
                        check(&format!("{kind} ink on popover"), ink, t.popover);
                    }
                    let warn_surface = composite(
                        t.warning.opacity(if dark { 0.16 } else { 0.08 }),
                        t.background,
                    );
                    check(
                        "warning button text",
                        super::ink(t, t.warning),
                        warn_surface,
                    );
                    check("link on background", t.link, t.background);
                    check("link on card", t.link, t.popover);
                    check_at(3.0, "focus ring on background", t.ring, t.background);
                    check_at(3.0, "focus ring on card", t.ring, t.popover);
                }
            }
        }
        assert!(low.is_empty(), "below the floor:\n{}", low.join("\n"));
    });
}
