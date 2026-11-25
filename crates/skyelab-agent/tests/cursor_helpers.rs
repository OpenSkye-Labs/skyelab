//! Acceptance: Cursor helpers (`login_status`, `list_cursor_models`, `account_default_model`, `generate_title`,
//! `generate_commit_message`, `generate_pr`, `usage_limits`, and `import`) work cleanly under `CURSOR_CONFIG_DIR`.
#![cfg(unix)]

use skyelab_agent::{
    Limits, Login, account_default_model, commit_prompt, generate_commit_message, generate_pr,
    generate_title, import, list_cursor_models, login_status, pr_prompt, title_prompt,
    usage_limits,
};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::{Path, PathBuf};

fn fake() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-cursor.sh")
}

fn cursor_account(dir: &Path) -> Account {
    Account {
        id: "cur".into(),
        label: "Cursor".into(),
        provider: Provider::Cursor,
        config_dir: dir.to_owned(),
        gh_config_dir: None,
        ssh_agent: false,
    }
}

#[test]
fn cursor_login_status_checks_config_dir_only() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let missing = tmp.path().join("missing");
    assert_eq!(
        login_status(&fake(), &cursor_account(&missing)).expect("missing dir"),
        Login::SignedOut
    );
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    std::fs::create_dir_all(&a).expect("mkdir");
    std::fs::create_dir_all(&b).expect("mkdir");
    std::fs::write(a.join("fake-login"), "dev@cursor.example\n").expect("write");
    assert_eq!(
        login_status(&fake(), &cursor_account(&a)).expect("signed in"),
        Login::SignedIn(Some("dev@cursor.example".into()))
    );
    assert_eq!(
        login_status(&fake(), &cursor_account(&b)).expect("signed out"),
        Login::SignedOut
    );
}

#[test]
fn cursor_models_and_default_model_filter_bad_names() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonical");
    let acct_dir = dir.join("cur-home");
    std::fs::create_dir_all(&acct_dir).expect("mkdir");
    let account = cursor_account(&acct_dir);
    let config = AccountsConfig {
        accounts: vec![account.clone()],
        bindings: vec![Binding::new(dir.clone(), "cur".into())],
    };

    let models = list_cursor_models(&fake(), &config, &account, &dir).expect("models");
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["auto", "composer-1", "claude-4-sonnet"]);
    assert!(models[0].is_default, "auto is marked default");

    std::fs::write(
        acct_dir.join("cli-config.json"),
        r#"{"model":{"modelId":"composer-1"}}"#,
    )
    .expect("write");
    assert_eq!(
        account_default_model(&account).as_deref(),
        Some("composer-1")
    );
    std::fs::write(acct_dir.join("cli-config.json"), r#"{"model":"--evil"}"#).expect("write");
    assert_eq!(account_default_model(&account), None);
    std::fs::remove_file(acct_dir.join("cli-config.json")).expect("rm");
    std::fs::create_dir(acct_dir.join("cli-config.json")).expect("dir instead of file");
    assert_eq!(account_default_model(&account), None);
}

#[test]
fn cursor_one_shot_title_commit_pr_usage_and_import() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonical");
    let acct_dir = dir.join("cur-home");
    std::fs::create_dir_all(&acct_dir).expect("mkdir");
    let account = cursor_account(&acct_dir);
    let config = AccountsConfig {
        accounts: vec![account.clone()],
        bindings: vec![Binding::new(dir.clone(), "cur".into())],
    };

    let t =
        generate_title(&fake(), &config, &account, &dir, &title_prompt("hi", "yo")).expect("title");
    assert_eq!(t, "Cursor fake title");
    let msg = generate_commit_message(
        &fake(),
        &config,
        &account,
        &dir,
        &commit_prompt("t", "p", "+a"),
    )
    .expect("commit");
    assert_eq!(msg, "fix(cursor): commit from cursor");
    let (pr_title, pr_body) = generate_pr(
        &fake(),
        &config,
        &account,
        &dir,
        &pr_prompt("t", "p", "main", "- c", "+a", None),
    )
    .expect("pr");
    assert_eq!(pr_title, "fix(cursor): pull request");
    assert_eq!(pr_body, "Cursor PR body.");

    let log = std::fs::read_to_string(dir.join(".fake-title-call")).expect("log");
    assert!(log.contains(&format!("cursor_config_dir={}", acct_dir.display())));
    assert!(log.contains("arg=[--mode]") && log.contains("arg=[ask]"));

    assert_eq!(
        usage_limits(&fake(), &account).expect("usage"),
        Limits::Unavailable
    );
    assert!(import::list(Provider::Cursor, &acct_dir, &dir).is_empty());
    assert!(import::project_folders(Provider::Cursor, &acct_dir).is_empty());
    assert!(
        import::read(
            Provider::Cursor,
            &acct_dir,
            &acct_dir.join("any.jsonl"),
            import::MAX_TRANSCRIPT_BYTES
        )
        .is_none()
    );
}
