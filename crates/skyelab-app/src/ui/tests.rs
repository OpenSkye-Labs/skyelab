use super::{Status, account_color, ago, short_path};
use gpui::TestAppContext;
use gpui_component::{ActiveTheme as _, Theme};
use std::path::Path;

/// The real default theme (`Theme::default()` has every color zeroed).
fn theme(cx: &mut TestAppContext) -> Theme {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.theme().clone()
    })
}

#[gpui::test]
fn account_color_is_stable_and_never_danger(cx: &mut TestAppContext) {
    let t = theme(cx);
    // Pinned: FNV-1a("work") picks the same palette slot in every session and on every OS.
    assert_eq!(
        account_color(&t, "work"),
        super::account_palette(&t)[super::fnv1a("work") as usize % 5]
    );
    assert_eq!(super::fnv1a("work"), 0x7058edf6366825c0);
    assert_eq!(
        super::fnv1a(""),
        0xcbf2_9ce4_8422_2325,
        "FNV-1a 64 offset basis"
    );
    assert_eq!(super::fnv1a("a"), 0xaf63_dc4c_8601_ec8c, "FNV-1a 64 of 'a'");
    let ids = [
        "work",
        "personal",
        "codex-lab",
        "client-a",
        "client-b",
        "oss",
        "acme",
        "home",
    ];
    let colors: Vec<_> = ids.iter().map(|id| account_color(&t, id)).collect();
    for c in &colors {
        assert_ne!(*c, t.danger, "an account never reads as an error");
        assert_ne!(*c, t.red);
    }
    let distinct = colors
        .iter()
        .enumerate()
        .filter(|(i, c)| !colors[..*i].contains(c))
        .count();
    assert!(
        distinct > 2,
        "ids spread over the palette, got {distinct} colors"
    );
}

#[test]
fn ago_formats() {
    let now = 1_000_000_000_000;
    let (s, m, h, d) = (1000, 60_000, 3_600_000, 86_400_000);
    assert_eq!(ago(now, now), "now");
    assert_eq!(
        ago(now, now + 5 * s),
        "now",
        "a clock skew into the future is now"
    );
    assert_eq!(ago(now, now - 59 * s), "now");
    assert_eq!(ago(now, now - 3 * m), "3m");
    assert_eq!(ago(now, now - 2 * h), "2h");
    assert_eq!(ago(now, now - 5 * d), "5d");
    assert_eq!(ago(now, now - 20 * d), "2w");
    assert_eq!(ago(now, now - 400 * d), "1y");
}

#[gpui::test]
fn status_parse_covers_store_values(cx: &mut TestAppContext) {
    for (raw, want) in [
        ("starting", Status::Starting),
        ("running", Status::Running),
        ("idle", Status::Idle),
        ("done", Status::Idle),
        ("stopped", Status::Stopped),
        ("exited", Status::Stopped),
        ("interrupted", Status::Interrupted),
        ("failed", Status::Failed),
        ("something-new", Status::Unknown),
    ] {
        assert_eq!(Status::parse(raw), want, "{raw}");
    }
    let t = theme(cx);
    assert_eq!(Status::Failed.color(&t), t.danger);
    assert_eq!(Status::Stale.color(&t), t.warning);
    assert_eq!(Status::Running.color(&t), t.info);
}

#[test]
fn short_path_abbreviates_home_only_on_a_component_boundary() {
    let home = Path::new("/home/dev");
    assert_eq!(
        short_path(Path::new("/home/dev/code/app"), Some(home)),
        "~/code/app"
    );
    assert_eq!(short_path(Path::new("/home/dev"), Some(home)), "~");
    assert_eq!(
        short_path(Path::new("/home/devops/app"), Some(home)),
        "/home/devops/app"
    );
    assert_eq!(short_path(Path::new("/srv/app"), None), "/srv/app");
}

#[gpui::test]
fn skyelab_theme_installs_for_both_modes(cx: &mut TestAppContext) {
    let (light, dark) = cx.update(|cx| {
        gpui_component::init(cx);
        super::install_theme(cx).expect("skyelab.json parses and names both themes");
        Theme::change(gpui_component::ThemeMode::Light, None, cx);
        let light = cx.theme().clone();
        Theme::change(gpui_component::ThemeMode::Dark, None, cx);
        (light, cx.theme().clone())
    });
    assert_eq!(light.light_theme.name, "SkyeLab Light");
    assert_eq!(dark.dark_theme.name, "SkyeLab Dark");
    // T3's canvas: near-white in light, true black in dark.
    assert!(
        light.background.l > 0.98,
        "light canvas {:?}",
        light.background
    );
    assert!(
        dark.background.l < 0.01,
        "dark canvas {:?}",
        dark.background
    );
    assert_ne!(
        light.sidebar, light.background,
        "the sidebar is one step off the canvas"
    );
    // Every dialog's backdrop: the canvas, dense enough to push the page back without a blur.
    for t in [&light, &dark] {
        assert!(t.overlay.a >= 0.7, "overlay {:?}", t.overlay);
        assert!((t.overlay.l - t.background.l).abs() < 0.01, "overlay is the canvas color");
    }
}

#[test]
fn configured_accounts_get_distinct_colors_in_any_order() {
    // "work" and "codex-lab" share a hash slot; configured together they must not share a color.
    assert_eq!(super::fnv1a("work") % 5, super::fnv1a("codex-lab") % 5);
    let a = super::account_slots(["work", "codex-lab", "personal"]);
    let b = super::account_slots(["personal", "codex-lab", "work"]);
    assert_eq!(a, b, "the order in accounts.toml doesn't matter");
    assert_ne!(a["work"], a["codex-lab"]);
    let many: Vec<String> = (0..12).map(|i| format!("acct-{i}")).collect();
    let slots = super::account_slots(many.iter().map(String::as_str));
    for chunk in 0..5 {
        assert!(
            slots.values().any(|s| *s == chunk),
            "every color used before any repeats"
        );
    }
}
