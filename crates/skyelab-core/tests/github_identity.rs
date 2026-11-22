//! Acceptance: each account may carry its own `gh` config dir, checked like `config_dir`.

use skyelab_core::{Account, AccountsConfig, Binding};
use std::path::{Path, PathBuf};

fn acct(id: &str, gh: Option<&str>) -> Account {
    Account {
        id: id.into(),
        label: id.into(),
        config_dir: PathBuf::from(format!("/home/me/.claude-{id}")),
        gh_config_dir: gh.map(PathBuf::from),
    }
}

/// `work` bound to `account`, in a temp dir that exists (bindings snapshot the real folder).
fn bound(account: Account) -> (tempfile::TempDir, PathBuf, AccountsConfig) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let work = std::fs::canonicalize(tmp.path())
        .expect("canonical")
        .join("work");
    std::fs::create_dir(&work).expect("mkdir");
    let mut c = AccountsConfig::default();
    let id = account.id.clone();
    c.add_account(account).expect("add");
    c.add_binding(Binding::new(work.clone(), id)).expect("bind");
    (tmp, work, c)
}

#[test]
fn relative_gh_dir_is_refused_everywhere() {
    let mut c = AccountsConfig::default();
    let err = c
        .add_account(acct("acme", Some("rel/gh")))
        .expect_err("relative gh dir");
    assert!(format!("{err:#}").contains("acme"), "{err:#}");

    c.add_account(acct("acme", None))
        .expect("no gh dir is fine");
    assert!(c.update_account(acct("acme", Some("rel/gh"))).is_err());

    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("accounts.toml");
    std::fs::write(
        &path,
        "[[account]]\nid = \"acme\"\nlabel = \"A\"\nconfig_dir = \"/c\"\ngh_config_dir = \"gh\"\n",
    )
    .expect("write");
    assert!(AccountsConfig::load(&path).is_err());

    // Public fields: an unvalidated account still can't launch with a relative gh dir.
    let (_tmp, work, mut c) = bound(acct("acme", Some("/abs/gh")));
    c.accounts[0].gh_config_dir = Some("gh".into());
    assert!(c.launch_env(&work).is_err());
}

#[test]
fn gh_dir_round_trips_and_absent_stays_absent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("accounts.toml");
    let mut c = AccountsConfig::default();
    c.add_account(acct("acme", Some("/home/me/.gh-acme")))
        .expect("add");
    c.add_account(acct("personal", None)).expect("add");
    c.save(&path).expect("save");
    let text = std::fs::read_to_string(&path).expect("read");
    assert_eq!(text.matches("gh_config_dir").count(), 1, "{text}");
    assert_eq!(AccountsConfig::load(&path).expect("load"), c);
}

#[test]
fn launch_env_sets_gh_config_dir_only_when_present() {
    let (_tmp, work, c) = bound(acct("acme", Some("/home/me/.gh-acme")));
    let env = c.launch_env(&work).expect("env");
    assert!(env.contains(&("CLAUDE_CONFIG_DIR", PathBuf::from("/home/me/.claude-acme"))));
    assert!(env.contains(&("GH_CONFIG_DIR", PathBuf::from("/home/me/.gh-acme"))));

    let (_tmp, work, c) = bound(acct("acme", None));
    let env = c.launch_env(&work).expect("env");
    assert_eq!(env.len(), 1, "{env:?}");
    assert_eq!(env[0].0, "CLAUDE_CONFIG_DIR");
}

#[test]
fn github_config_dir_refuses_without_identity_or_binding() {
    let (_tmp, work, c) = bound(acct("acme", Some("/home/me/.gh-acme")));
    assert_eq!(
        c.github_config_dir(&work.join("sub")).expect("bound"),
        Path::new("/home/me/.gh-acme")
    );

    let (_tmp, work, c) = bound(acct("acme", None));
    let err = c.github_config_dir(&work).expect_err("no identity");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("no GitHub login") && msg.contains("acme"),
        "{msg}"
    );

    let err = c
        .github_config_dir(Path::new("/nonexistent/elsewhere"))
        .expect_err("unbound");
    assert!(format!("{err:#}").contains("not bound"), "{err:#}");
}
