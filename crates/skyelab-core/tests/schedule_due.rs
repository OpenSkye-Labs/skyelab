//! Acceptance 2, 3, 6 (core rules): what a tick does with a due task, the account a run may use, and the Full access
//! confirm.

use skyelab_core::schedule::{Due, MISSED_GRACE_MS, check_mode, due, run_account};
use skyelab_core::{Account, AccountsConfig, Binding};

const NOW: i64 = 1_800_000_000_000;

#[test]
fn a_task_runs_when_due() {
    assert_eq!(due(None, NOW, false), Due::NotYet, "never scheduled");
    assert_eq!(due(Some(NOW + 1), NOW, false), Due::NotYet);
    assert_eq!(due(Some(NOW), NOW, false), Due::Run);
    assert_eq!(
        due(Some(NOW - MISSED_GRACE_MS), NOW, true),
        Due::Run,
        "late but in grace"
    );
}

#[test]
fn a_missed_run_runs_once_or_is_skipped_per_setting() {
    let missed = NOW - MISSED_GRACE_MS - 1;
    assert_eq!(due(Some(missed), NOW, false), Due::Run, "default: run once");
    assert_eq!(due(Some(missed), NOW, true), Due::SkipMissed);
    // Three slots missed is still one decision, not three.
    assert_eq!(due(Some(NOW - 3 * 3_600_000), NOW, false), Due::Run);
}

#[test]
fn full_access_needs_the_tasks_own_confirm() {
    assert!(check_mode("bypass", false).is_err());
    assert!(check_mode("bypass", true).is_ok());
    for mode in ["default", "plan", "accept_edits"] {
        assert!(check_mode(mode, false).is_ok(), "{mode}");
    }
}

fn config(dir: &std::path::Path, bound_to: Option<&str>) -> AccountsConfig {
    let mut c = AccountsConfig::default();
    for id in ["acme", "home"] {
        c.add_account(Account {
            id: id.into(),
            label: id.into(),
            config_dir: dir.join(format!(".claude-{id}")),
            provider: Default::default(),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("account");
    }
    if let Some(id) = bound_to {
        c.add_binding(Binding::new(dir.join("work"), id.into()))
            .expect("bind");
    }
    c
}

#[test]
fn a_run_uses_the_folders_account_only_when_it_is_the_saved_one() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = std::fs::canonicalize(tmp.path()).expect("canon");
    std::fs::create_dir_all(root.join("work")).expect("dir");
    let folder = root.join("work");

    let same = config(&root, Some("acme"));
    assert_eq!(
        run_account(&same, &folder, "acme").expect("runs").id,
        "acme"
    );

    let rebound = config(&root, Some("home"));
    let why = run_account(&rebound, &folder, "acme").expect_err("rebound");
    assert!(why.contains("home") && why.contains("acme"), "{why}");

    let unbound = config(&root, None);
    let why = run_account(&unbound, &folder, "acme").expect_err("unbound");
    assert!(why.contains("isn't bound"), "{why}");

    // An unreadable accounts.toml reaches here as an empty config: nothing runs.
    assert!(run_account(&AccountsConfig::default(), &folder, "acme").is_err());
}
