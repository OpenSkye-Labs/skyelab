#![cfg(unix)]
use skyelab_core::{Account, AccountsConfig, Binding};
use std::os::unix::fs::symlink;

#[test]
fn binding_the_old_target_of_a_stale_link_is_not_silently_dead() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let real = root.join("real");
    let other = root.join("other");
    let link = root.join("link");
    std::fs::create_dir(&real).expect("mkdir");
    std::fs::create_dir(&other).expect("mkdir");
    symlink(&real, &link).expect("symlink");
    let mut c = AccountsConfig::default();
    for id in ["acme", "personal"] {
        c.add_account(Account {
            id: id.into(),
            label: id.into(),
            config_dir: format!("/home/me/.claude-{id}").into(),
        })
        .expect("account");
    }
    c.add_binding(Binding::new(link.clone(), "personal".into()))
        .expect("bind link");
    std::fs::remove_file(&link).expect("rm link");
    symlink(&other, &link).expect("retarget");

    let added = c.add_binding(Binding::new(real.clone(), "acme".into()));
    let got = c.resolve(&real).map(|a| a.id.clone());
    assert!(
        added.is_err() || got.as_deref() == Some("acme"),
        "SILENT_DEAD_BINDING: add_binding accepted {} but resolve gives {got:?}",
        real.display()
    );
}
