//! Regression tests for review findings on save and edits.

#[cfg(unix)]
use skyelab_core::Binding;
use skyelab_core::{Account, AccountsConfig};
use std::path::PathBuf;

/// A Unix-style absolute path, on drive C: on Windows so it is absolute there too.
fn abs(p: &str) -> PathBuf {
    if cfg!(windows) && p.starts_with('/') {
        format!("C:{p}").into()
    } else {
        p.into()
    }
}

fn acct(id: &str, dir: &str) -> Account {
    Account {
        id: id.into(),
        label: id.into(),
        config_dir: abs(dir),
        provider: Default::default(),
        gh_config_dir: None,
        ssh_agent: false,
    }
}

#[test]
fn empty_or_relative_config_dir_is_rejected() {
    let mut c = AccountsConfig::default();
    assert!(
        c.add_account(acct("a", "")).is_err(),
        "empty config_dir accepted"
    );
    assert!(
        c.add_account(acct("b", "~/.claude-b")).is_err(),
        "relative config_dir accepted"
    );
    c.add_account(acct("c", "/home/me/.claude-c"))
        .expect("absolute ok");
    assert!(
        c.update_account(acct("c", "")).is_err(),
        "update to empty accepted"
    );
    assert_eq!(c.accounts[0].config_dir, abs("/home/me/.claude-c"));
}

#[cfg(unix)]
#[test]
fn save_keeps_a_symlinked_config_file_linked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let real = dir.path().join("dotfiles-accounts.toml");
    let link = dir.path().join("accounts.toml");
    std::fs::write(&real, "").expect("write");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");
    let mut c = AccountsConfig::default();
    c.add_account(acct("a", "/home/me/.claude-a")).expect("add");
    c.save(&link).expect("save");
    assert!(
        std::fs::symlink_metadata(&link)
            .expect("meta")
            .file_type()
            .is_symlink()
    );
    assert_eq!(AccountsConfig::load(&real).expect("load real"), c);
}

#[cfg(unix)]
#[test]
fn binding_the_same_folder_via_a_symlink_is_a_duplicate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let real = dir.path().join("work");
    let link = dir.path().join("work-link");
    std::fs::create_dir(&real).expect("mkdir");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");
    let mut c = AccountsConfig::default();
    c.add_account(acct("a", "/home/me/.claude-a"))
        .expect("add a");
    c.add_account(acct("b", "/home/me/.claude-b"))
        .expect("add b");
    c.add_binding(Binding::new(real, "a".into())).expect("bind");
    assert!(c.add_binding(Binding::new(link, "b".into())).is_err());
}
