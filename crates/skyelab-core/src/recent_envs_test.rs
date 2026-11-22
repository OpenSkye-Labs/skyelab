//! Recent remote environments in settings.toml.

use crate::AppSettings;

#[test]
fn remember_environment_moves_to_front_without_duplicates_and_caps() {
    let mut s = AppSettings::default();
    s.remember_environment("a.ts.net");
    s.remember_environment("b.ts.net");
    s.remember_environment("A.ts.net");
    assert_eq!(s.recent_environments, ["A.ts.net", "b.ts.net"]);
    for i in 0..20 {
        s.remember_environment(&format!("h{i}"));
    }
    assert_eq!(s.recent_environments.len(), crate::MAX_RECENT_ENVIRONMENTS);
    assert_eq!(s.recent_environments[0], "h19");
    s.remember_environment("  ");
    assert_eq!(s.recent_environments[0], "h19", "blank names are ignored");
}

#[test]
fn recents_round_trip_and_old_files_still_load() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "share_on_tailnet = true\n").expect("write");
    let mut s = AppSettings::load(&path).expect("an older file loads");
    assert!(s.share_on_tailnet && s.recent_environments.is_empty());
    s.remember_environment("laptop.ts.net");
    s.save(&path).expect("save");
    let back = AppSettings::load(&path).expect("load");
    assert_eq!(back, s);
}
