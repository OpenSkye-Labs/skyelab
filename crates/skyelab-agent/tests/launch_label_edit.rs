//! The spawn re-check compares the login an account is (id, provider, config dir, gh dir, ssh), not its
//! display label: a label edited in accounts.toml must not refuse a launch, a changed login still must.
#![cfg(unix)]

use skyelab_agent::{Launch, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding};
use std::path::{Path, PathBuf};

fn fake_bin() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh")
}

fn config(dir: &Path, account: Account) -> AccountsConfig {
    AccountsConfig {
        accounts: vec![account],
        bindings: vec![Binding::new(dir.into(), "work".into())],
    }
}

fn start(saved: &Account, launched: &Account, dir: &Path) -> anyhow::Result<()> {
    let launch = Launch {
        account: launched,
        dir,
        resume: None,
        prompt: "hi",
        mcp_config: None,
        model: None,
        effort: None,
        attachments: &[],
        mode: Default::default(),
    };
    let mut session = start_with_bin(&fake_bin(), &config(dir, saved.clone()), &launch)?;
    session.kill()
}

#[test]
fn a_label_only_edit_still_launches_and_a_changed_login_does_not() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let launched = Account {
        id: "work".into(),
        label: "Work".into(),
        config_dir: dir.join(".claude-work"),
        provider: Default::default(),
        gh_config_dir: None,
        ssh_agent: false,
    };
    let relabeled = Account {
        label: "Work (renamed)".into(),
        ..launched.clone()
    };
    start(&relabeled, &launched, &dir).expect("a new label is the same login");

    let moved = Account {
        config_dir: dir.join(".claude-elsewhere"),
        ..launched.clone()
    };
    assert!(
        start(&moved, &launched, &dir).is_err(),
        "another config dir"
    );
    let ssh = Account {
        ssh_agent: true,
        ..launched.clone()
    };
    assert!(start(&ssh, &launched, &dir).is_err(), "ssh turned on");
    let gh = Account {
        gh_config_dir: Some(dir.join("gh")),
        ..launched.clone()
    };
    assert!(start(&gh, &launched, &dir).is_err(), "another gh login");
    let codex = Account {
        provider: skyelab_core::Provider::Codex,
        ..launched.clone()
    };
    assert!(start(&codex, &launched, &dir).is_err(), "another provider");
}
