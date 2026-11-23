#![cfg(unix)]
use skyelab_core::{Account, AccountsConfig, Binding};

#[test]
fn reconfirm_refuses_a_folder_another_binding_already_owns() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let real = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let mut c = AccountsConfig::default();
    for id in ["a", "b"] {
        c.add_account(Account {
            id: id.into(),
            label: id.into(),
            config_dir: format!("/abs/{id}").into(),
            provider: Default::default(),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("account");
    }
    let d = real.join("d");
    let l = real.join("l");
    c.add_binding(Binding::new(d.clone(), "a".into()))
        .expect("bind d");
    c.add_binding(Binding::new(l.clone(), "b".into()))
        .expect("bind l");
    std::fs::create_dir(&d).expect("mkdir");
    std::os::unix::fs::symlink(&d, &l).expect("symlink");
    assert!(
        c.reconfirm_binding(&l, &d).is_err(),
        "RECONFIRMED_DUPLICATE_ROOT"
    );
}
