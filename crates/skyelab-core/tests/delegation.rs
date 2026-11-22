//! The delegation rule: a delegated thread runs under its target's bound account, and only when that is the caller's.

use skyelab_core::delegation::{MAX_CHILDREN, MAX_DEPTH, check_limits};
use skyelab_core::{Account, AccountsConfig, Binding};
use std::path::{Path, PathBuf};

struct Fixture {
    _tmp: tempfile::TempDir,
    config: AccountsConfig,
    work_a: PathBuf,
    work_b: PathBuf,
    personal: PathBuf,
    unbound: PathBuf,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let dir = |name: &str| {
        let d = root.join(name);
        std::fs::create_dir_all(&d).expect("mkdir");
        d
    };
    let (work_a, work_b, personal, unbound) = (
        dir("work-a"),
        dir("work-b"),
        dir("personal"),
        dir("unbound"),
    );
    let mut config = AccountsConfig::default();
    for (id, label) in [("work", "Work"), ("me", "Personal")] {
        config
            .add_account(Account {
                id: id.into(),
                label: label.into(),
                config_dir: root.join(format!(".claude-{id}")),
                gh_config_dir: None,
            })
            .expect("account");
    }
    for (d, id) in [(&work_a, "work"), (&work_b, "work"), (&personal, "me")] {
        config
            .add_binding(Binding::new(d.clone(), id.into()))
            .expect("bind");
    }
    Fixture {
        _tmp: tmp,
        config,
        work_a,
        work_b,
        personal,
        unbound,
    }
}

#[test]
fn same_account_target_is_allowed_under_that_account() {
    let f = fixture();
    let account = f
        .config
        .delegation_account("work", &f.work_b)
        .expect("same account is allowed");
    assert_eq!(account.id, "work");
    assert_eq!(
        f.config
            .delegation_account("work", &f.work_a)
            .expect("own project")
            .id,
        "work"
    );
}

#[test]
fn cross_account_target_is_refused_naming_both_accounts() {
    let f = fixture();
    let err = f
        .config
        .delegation_account("work", &f.personal)
        .expect_err("cross-account must be refused");
    assert!(err.contains("work") && err.contains("me"), "{err}");
    let err = f
        .config
        .delegation_account("me", &f.work_a)
        .expect_err("and the reverse");
    assert!(err.contains("work") && err.contains("me"), "{err}");
}

#[test]
fn unbound_target_is_refused() {
    let f = fixture();
    let err = f
        .config
        .delegation_account("work", &f.unbound)
        .expect_err("unbound must be refused, never a default account");
    assert!(err.contains("not bound"), "{err}");
    assert!(
        f.config
            .delegation_account("work", Path::new("relative"))
            .is_err()
    );
}

#[test]
fn an_unknown_caller_account_is_refused() {
    let f = fixture();
    assert!(f.config.delegation_account("", &f.work_a).is_err());
    assert!(f.config.delegation_account("ghost", &f.work_a).is_err());
}

#[test]
fn depth_and_children_limits() {
    assert_eq!(MAX_DEPTH, 2);
    assert_eq!(MAX_CHILDREN, 4);
    check_limits(0, 0).expect("a user thread may delegate");
    check_limits(MAX_DEPTH - 1, MAX_CHILDREN - 1).expect("just under both limits");
    let err = check_limits(MAX_DEPTH, 0).expect_err("too deep");
    assert!(err.contains("depth"), "{err}");
    let err = check_limits(0, MAX_CHILDREN).expect_err("too many children");
    assert!(err.contains("children"), "{err}");
    assert!(check_limits(MAX_DEPTH + 5, MAX_CHILDREN + 5).is_err());
}

/// Driving (status, read, wait, send) needs the thread recorded under the caller's account AND its project still
/// bound to that account.
#[test]
fn drive_needs_the_same_account_on_the_thread_and_its_project() {
    let f = fixture();
    f.config
        .check_drive("work", "work", &f.work_b)
        .expect("same account thread in a same account project");
    let err = f
        .config
        .check_drive("work", "me", &f.personal)
        .expect_err("another account's thread");
    assert!(err.contains("me"), "{err}");
    f.config
        .check_drive("work", "me", &f.work_a)
        .expect_err("recorded under another account, even in a work project");
    f.config
        .check_drive("work", "work", &f.personal)
        .expect_err("recorded under work but its project is now bound to me");
    f.config
        .check_drive("work", "work", &f.unbound)
        .expect_err("project unbound now");
}
