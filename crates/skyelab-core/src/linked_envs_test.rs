//! Linked machines: the names whose threads show in this machine's list.

use crate::AppSettings;

#[test]
fn linked_envs_toggle_case_insensitive_and_round_trip() {
    let mut s = AppSettings::default();
    assert!(s.toggle_linked_environment(" desk.ts.net "), "linked");
    assert!(s.is_linked_environment("DESK.ts.net"));
    assert!(s.toggle_linked_environment("laptop.ts.net"));
    assert!(
        !s.toggle_linked_environment("Desk.TS.net"),
        "same name, other case: unlinked"
    );
    assert_eq!(s.linked_environments, ["laptop.ts.net"]);
    assert!(
        !s.toggle_linked_environment("  "),
        "blank names are ignored"
    );
    assert_eq!(s.linked_environments, ["laptop.ts.net"]);

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "share_on_tailnet = true\n").expect("write");
    let old = AppSettings::load(&path).expect("an older file loads");
    assert!(old.linked_environments.is_empty());
    s.save(&path).expect("save");
    assert_eq!(AppSettings::load(&path).expect("load"), s);
}
