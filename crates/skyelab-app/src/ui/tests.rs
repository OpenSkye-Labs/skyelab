use super::{Status, account_color, ago, short_path};
use gpui_component::Theme;
use std::path::Path;

fn theme() -> Theme {
    Theme::default()
}

#[test]
fn account_color_is_stable_and_never_danger() {
    let t = theme();
    // Pinned value: FNV-1a("work") picks the same palette slot in every session and on every OS.
    assert_eq!(account_color(&t, "work"), account_color(&t, "work"));
    assert_eq!(account_color(&t, "work"), super::account_palette(&t)[super::fnv1a("work") as usize % 5]);
    assert_eq!(super::fnv1a("work"), 0x7058edf6366825c0);
    assert_eq!(super::fnv1a(""), 0xcbf2_9ce4_8422_2325, "FNV-1a 64 offset basis");
    assert_eq!(super::fnv1a("a"), 0xaf63_dc4c_8601_ec8c, "FNV-1a 64 of 'a'");
    let ids = ["work", "personal", "codex-lab", "client-a", "client-b", "oss", "acme", "home"];
    let colors: Vec<_> = ids.iter().map(|id| account_color(&t, id)).collect();
    for c in &colors {
        assert_ne!(*c, t.danger, "an account never reads as an error");
        assert_ne!(*c, t.red);
    }
    let distinct = colors.iter().enumerate().filter(|(i, c)| !colors[..*i].contains(c)).count();
    assert!(distinct > 2, "ids spread over the palette, got {distinct} colors");
}

#[test]
fn ago_formats() {
    let now = 1_000_000_000_000;
    let (s, m, h, d) = (1000, 60_000, 3_600_000, 86_400_000);
    assert_eq!(ago(now, now), "now");
    assert_eq!(ago(now, now + 5 * s), "now", "a clock skew into the future is now");
    assert_eq!(ago(now, now - 59 * s), "now");
    assert_eq!(ago(now, now - 3 * m), "3m");
    assert_eq!(ago(now, now - 2 * h), "2h");
    assert_eq!(ago(now, now - 5 * d), "5d");
    assert_eq!(ago(now, now - 20 * d), "2w");
    assert_eq!(ago(now, now - 400 * d), "1y");
}

#[test]
fn status_parse_covers_store_values() {
    for (raw, want) in [
        ("starting", Status::Starting),
        ("running", Status::Running),
        ("idle", Status::Idle),
        ("done", Status::Idle),
        ("stopped", Status::Stopped),
        ("interrupted", Status::Interrupted),
        ("failed", Status::Failed),
        ("something-new", Status::Unknown),
    ] {
        assert_eq!(Status::parse(raw), want, "{raw}");
    }
    let t = theme();
    assert_eq!(Status::Failed.color(&t), t.danger);
    assert_eq!(Status::Refused.color(&t), t.danger);
    assert_eq!(Status::Stale.color(&t), t.warning);
    assert_eq!(Status::Running.color(&t), t.info);
}

#[test]
fn short_path_abbreviates_home_only_on_a_component_boundary() {
    let home = Path::new("/home/dev");
    assert_eq!(short_path(Path::new("/home/dev/code/app"), Some(home)), "~/code/app");
    assert_eq!(short_path(Path::new("/home/dev"), Some(home)), "~");
    assert_eq!(short_path(Path::new("/home/devops/app"), Some(home)), "/home/devops/app");
    assert_eq!(short_path(Path::new("/srv/app"), None), "/srv/app");
}
