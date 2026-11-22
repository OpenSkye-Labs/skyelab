#![cfg(unix)]
use skyelab_core::{Account, AccountsConfig, Binding};

#[test]
fn bindings_that_become_the_same_folder_do_not_silently_pick_one() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let real = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let mut c = AccountsConfig::default();
    for id in ["a", "b"] {
        c.add_account(Account {
            id: id.into(),
            label: id.into(),
            config_dir: format!("/abs/{id}").into(),
        })
        .expect("account");
    }
    let d = real.join("d");
    let l = real.join("l");
    c.add_binding(Binding::new(d.clone(), "a".into()))
        .expect("bind d");
    c.add_binding(Binding::new(l.clone(), "b".into()))
        .expect("bind l (not yet an alias)");
    std::fs::create_dir(&d).expect("mkdir");
    std::os::unix::fs::symlink(&d, &l).expect("symlink");
    assert_eq!(
        c.resolve(&d).map(|a| a.id.as_str()),
        None,
        "AMBIGUOUS_TIE_PICKED_AN_ACCOUNT"
    );
}
