#![cfg(unix)]
use skyelab_core::{Account, AccountsConfig, Binding};

#[test]
fn reconfirm_refuses_a_folder_swapped_for_a_link_to_the_disk_root() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let acme = root.join("acme");
    std::fs::create_dir(&acme).expect("mkdir");
    let mut c = AccountsConfig::default();
    c.add_account(Account {
        id: "acme".into(),
        label: "Acme".into(),
        config_dir: "/home/me/.claude-acme".into(),
    })
    .expect("account");
    c.add_binding(Binding::new(acme.clone(), "acme".into()))
        .expect("bind");
    std::fs::remove_dir(&acme).expect("rm");
    std::os::unix::fs::symlink("/", &acme).expect("symlink");
    let _ = c.reconfirm_binding(&acme, std::path::Path::new("/")); // the fix makes this Err and leaves the binding stale
    assert_eq!(
        c.resolve(&root).map(|a| a.id.clone()),
        None,
        "RECONFIRM_CLAIMED_DISK"
    );
}
