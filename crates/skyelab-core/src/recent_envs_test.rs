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
fn forget_environment_drops_the_name_and_keeps_its_trusted_node() {
    let mut s = AppSettings::default();
    s.remember_environment("a.ts.net");
    s.remember_environment("b.ts.net");
    s.trust_environment_node("b.ts.net", "nB");
    s.forget_environment(" B.ts.net ");
    assert_eq!(s.recent_environments, ["a.ts.net"]);
    assert_eq!(
        s.environment_node("b.ts.net"),
        Some("nB"),
        "trust unchanged"
    );
    s.forget_environment("missing.ts.net");
    assert_eq!(s.recent_environments, ["a.ts.net"]);
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

#[test]
fn environment_nodes_are_kept_per_name_and_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    let mut s = AppSettings::default();
    assert_eq!(s.environment_node("laptop.tail1.ts.net"), None);
    s.trust_environment_node("Laptop.Tail1.ts.net", "nA");
    assert_eq!(
        s.environment_node("laptop.tail1.ts.net"),
        Some("nA"),
        "names compare as DNS does"
    );
    s.trust_environment_node("laptop.tail1.ts.net", "nB");
    assert_eq!(s.environment_nodes.len(), 1, "trusting again replaces");
    s.save(&path).expect("save");
    assert_eq!(AppSettings::load(&path).expect("load"), s);
}

#[test]
fn concurrent_updates_keep_every_write() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let path = path.clone();
            std::thread::spawn(move || {
                for j in 0..10 {
                    AppSettings::update(&path, |s| {
                        s.trust_environment_node(&format!("m{i}-{j}.t.ts.net"), "n")
                    })
                    .expect("update");
                }
            })
        })
        .collect();
    for t in threads {
        t.join().expect("thread");
    }
    let saved = AppSettings::load(&path).expect("load");
    assert_eq!(
        saved.environment_nodes.len(),
        80,
        "no write lost to a racing one"
    );
}

#[test]
fn window_layout_round_trips_and_older_files_still_load() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "share_on_tailnet = true\n").expect("write");
    assert_eq!(
        AppSettings::load(&path).expect("old file loads").window,
        None
    );
    let layout = crate::WindowLayout {
        x: -1200,
        y: 40,
        width: 1600,
        height: 1000,
        right_panel_open: true,
        right_panel_width: 480,
        view: "thread:7".into(),
        terminals: Vec::new(),
    };
    AppSettings::update(&path, |s| s.window = Some(layout.clone())).expect("save");
    let loaded = AppSettings::load(&path).expect("load");
    assert_eq!(loaded.window, Some(layout));
    assert!(loaded.share_on_tailnet, "other settings kept");
}
