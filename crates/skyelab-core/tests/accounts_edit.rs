//! Acceptance tests for editing and saving the accounts config.

use skyelab_core::{Account, AccountsConfig, Binding};
use std::path::{Path, PathBuf};

fn acct(id: &str) -> Account {
    Account {
        id: id.into(),
        label: format!("{id} label"),
        config_dir: PathBuf::from(format!("/home/me/.claude-{id}")),
        gh_config_dir: None,
    }
}

fn bind(path: &str, account: &str) -> Binding {
    Binding::new(path.into(), account.into())
}

fn two_accounts_one_binding() -> AccountsConfig {
    let mut c = AccountsConfig::default();
    c.add_account(acct("acme")).expect("add acme");
    c.add_account(acct("personal")).expect("add personal");
    c.add_binding(bind("/nonexistent/work/acme", "acme"))
        .expect("bind");
    c
}

#[test]
fn save_load_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("nested").join("accounts.toml");
    let c = two_accounts_one_binding();
    c.save(&path).expect("save creates parent dir");
    assert_eq!(AccountsConfig::load(&path).expect("load"), c);
    // No temp file left behind next to the config.
    let names: Vec<_> = std::fs::read_dir(path.parent().expect("parent"))
        .expect("read_dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("accounts.toml")]);
}

#[test]
fn save_rejects_invalid_config_and_keeps_old_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("accounts.toml");
    let good = two_accounts_one_binding();
    good.save(&path).expect("save");
    let mut bad = good.clone();
    bad.bindings[0].account = "typo".into();
    assert!(bad.save(&path).is_err());
    assert_eq!(AccountsConfig::load(&path).expect("load"), good);
}

#[test]
fn account_add_rejects_empty_and_duplicate_id() {
    let mut c = two_accounts_one_binding();
    let before = c.clone();
    assert!(c.add_account(acct("acme")).is_err());
    assert!(c.add_account(acct("  ")).is_err());
    assert_eq!(c, before);
}

#[test]
fn account_update_changes_label_and_dir_and_rejects_unknown() {
    let mut c = two_accounts_one_binding();
    let mut changed = acct("acme");
    changed.label = "Acme Team".into();
    changed.config_dir = "/elsewhere".into();
    c.update_account(changed.clone()).expect("update");
    assert_eq!(c.accounts[0], changed);

    let before = c.clone();
    assert!(c.update_account(acct("ghost")).is_err());
    assert_eq!(c, before);
}

#[test]
fn account_remove_refuses_while_bound_and_does_not_cascade() {
    let mut c = two_accounts_one_binding();
    let before = c.clone();
    let err = c
        .remove_account("acme")
        .expect_err("acme still has a binding");
    assert!(format!("{err:#}").contains("/nonexistent/work/acme"));
    assert_eq!(c, before);
    assert!(c.remove_account("ghost").is_err());

    c.remove_account("personal")
        .expect("unbound account removes");
    assert_eq!(c.accounts.len(), 1);
    assert_eq!(c.bindings.len(), 1);
}

#[test]
fn binding_add_rejects_unknown_account_and_duplicate_path() {
    let mut c = two_accounts_one_binding();
    let before = c.clone();
    assert!(c.add_binding(bind("/nonexistent/other", "typo")).is_err());
    assert!(
        c.add_binding(bind("/nonexistent/work/acme", "personal"))
            .is_err()
    );
    assert_eq!(c, before);

    c.add_binding(bind("/nonexistent/work/acme/fork", "personal"))
        .expect("nested binding");
    assert_eq!(
        c.resolve(Path::new("/nonexistent/work/acme/fork/src"))
            .map(|a| a.id.as_str()),
        Some("personal")
    );
}

#[test]
fn binding_remove_unbinds_and_rejects_unknown_path() {
    let mut c = two_accounts_one_binding();
    assert!(c.remove_binding(Path::new("/nonexistent/nope")).is_err());
    c.remove_binding(Path::new("/nonexistent/work/acme"))
        .expect("remove");
    assert!(c.resolve(Path::new("/nonexistent/work/acme")).is_none());
    c.remove_account("acme").expect("now unbound, removable");
}
