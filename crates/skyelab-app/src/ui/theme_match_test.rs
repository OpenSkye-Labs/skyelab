//! Own UI kit, stage 2a (.agent/plans/2026-11-27-own-ui-kit-theme.md), criterion 1: for every family, mode and accent,
//! our theme holds what gpui-component makes of the same theme file on its own (its `apply_config`, not the bridge),
//! and the live gpui-component theme its remaining widgets read holds the same.

use super::themes::{self, ACCENTS, FAMILIES};
use gpui::TestAppContext;
use skyelab_core::{AppearanceSettings, ColorMode};

macro_rules! same {
    ($ours:expr, $theirs:expr, $what:expr, $bad:expr; $($field:ident),* $(,)?) => {
        $(
            if $ours.$field != $theirs.$field {
                $bad.push(format!(
                    "{} {}: ours {:?}, gpui-component {:?}",
                    $what,
                    stringify!($field),
                    $ours.$field,
                    $theirs.$field
                ));
            }
        )*
    };
}

#[gpui::test]
fn our_theme_matches_gpui_components_for_every_choice(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        super::install_theme(cx).expect("every theme file loads");
    });
    let accents: Vec<Option<String>> = std::iter::once(None)
        .chain(ACCENTS.iter().map(|a| Some(a.key.to_string())))
        .collect();
    let mut bad = Vec::new();
    let mut checked = 0;
    for f in &FAMILIES {
        for accent in &accents {
            for mode in [ColorMode::Light, ColorMode::Dark] {
                cx.update(|cx| {
                    let s = AppearanceSettings {
                        mode,
                        theme: f.key.into(),
                        accent: accent.clone(),
                        ui_font_size: 15,
                        code_font_size: 12,
                        ..Default::default()
                    };
                    themes::set(s.clone(), cx).detach();
                    let dark = mode == ColorMode::Dark;
                    let ours = super::theme::Theme::global(cx);
                    let live = gpui_component::Theme::global(cx);
                    // gpui-component's own reading of the adjusted file, on a copy: the bridge can't write this one.
                    let config: gpui_component::ThemeConfig =
                        serde_json::from_value(themes::adjusted(f.key, &s, dark))
                            .expect("gpui-component reads the adjusted theme");
                    let mut theirs = live.clone();
                    theirs.apply_config(&std::rc::Rc::new(config));
                    let what = format!("{} {mode:?} accent {accent:?}", f.key);
                    same!(live, theirs, what, bad;
                        background, foreground, border, primary, danger_hover, sidebar_accent, tab_bar, selection,
                    );
                    same!(ours, theirs, what, bad;
                        background, foreground, border, muted, muted_foreground, accent, accent_foreground,
                        primary, primary_foreground, primary_hover, primary_active,
                        secondary, secondary_foreground, secondary_hover, secondary_active,
                        danger, danger_foreground, danger_hover, danger_active, warning, success, info,
                        input, popover, popover_foreground, ring, selection, overlay, link, tab_bar,
                        sidebar, sidebar_foreground, sidebar_accent, sidebar_accent_foreground, sidebar_border,
                        red, green, blue, yellow, magenta, cyan,
                        font_size, mono_font_size, radius_lg,
                    );
                    same!(ours, live, what, bad; font_family, mono_font_family);
                    if ours.is_dark() != theirs.is_dark() {
                        bad.push(format!("{what} is_dark: ours {}", ours.is_dark()));
                    }
                    let (oh, th) = (&ours.highlight_theme, &theirs.highlight_theme);
                    if oh.name != th.name || oh.appearance != th.appearance {
                        bad.push(format!("{what} highlight: ours {}, theirs {}", oh.name, th.name));
                    }
                    assert_eq!(ours.is_dark(), dark, "{what}: the mode shown");
                    assert_eq!(live.is_dark(), dark, "{what}: gpui-component's mode");
                });
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 14 * 7 * 2, "every family, accent and mode");
    assert!(
        bad.is_empty(),
        "{} differences:\n{}",
        bad.len(),
        bad.join("\n")
    );
}
