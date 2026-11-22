#![cfg(unix)]
use skyelab_core::{Account, AccountsConfig, Binding};

#[test]
fn bound_folder_swapped_for_a_dangling_link_is_stale() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let acme = root.join("acme");
    std::fs::create_dir(&acme).expect("mkdir");
    let mut c = AccountsConfig::default();
    c.add_account(Account {
        id: "acme".into(),
        label: "Acme".into(),
        config_dir: "/home/me/.claude-acme".into(),
        provider: Default::default(),
        gh_config_dir: None,
    })
    .expect("account");
    c.add_binding(Binding::new(acme.clone(), "acme".into()))
        .expect("bind");
    std::fs::remove_dir(&acme).expect("rm");
    std::os::unix::fs::symlink(root.join("not-there-yet"), &acme).expect("dangling symlink");
    assert!(c.bindings[0].is_stale(), "DANGLING_NOT_STALE");
    assert_eq!(
        c.resolve(&acme.join("x")).map(|a| a.id.clone()),
        None,
        "DANGLING_NOT_STALE"
    );
}
