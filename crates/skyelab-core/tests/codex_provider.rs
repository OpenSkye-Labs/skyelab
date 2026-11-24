//! Acceptance: an account is a Claude or a Codex login; a missing provider means Claude, and
//! `launch_env` sets only the bound provider's home var.

use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::PathBuf;

const MIXED: &str = r#"
[[account]]
id = "work"
label = "Work"
config_dir = "/home/me/.claude-work"

[[account]]
id = "oai"
label = "OpenAI"
provider = "codex"
config_dir = "/home/me/.codex-oai"
gh_config_dir = "/home/me/.config/gh-oai"
"#;

/// A Unix-style absolute path, on drive C: on Windows so it is absolute there too.
fn abs(p: &str) -> PathBuf {
    if cfg!(windows) && p.starts_with('/') {
        format!("C:{p}").into()
    } else {
        p.into()
    }
}

fn load(text: &str) -> anyhow::Result<AccountsConfig> {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("accounts.toml");
    let text = text.replace("\"/", if cfg!(windows) { "\"C:/" } else { "\"/" });
    std::fs::write(&path, text).expect("write config");
    AccountsConfig::load(&path)
}

#[test]
fn mixed_config_loads_and_missing_provider_is_claude() {
    let c = load(MIXED).expect("load");
    assert_eq!(c.accounts[0].provider, Provider::Claude);
    assert_eq!(c.accounts[1].provider, Provider::Codex);
}

#[test]
fn unknown_provider_is_a_load_error() {
    let err = load(&MIXED.replace("\"codex\"", "\"gemini\"")).expect_err("unknown provider");
    assert!(format!("{err:#}").contains("gemini"), "{err:#}");
}

#[test]
fn saving_writes_provider_only_for_codex() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("accounts.toml");
    load(MIXED).expect("load").save(&path).expect("save");
    let text = std::fs::read_to_string(&path).expect("read");
    assert_eq!(text.matches("provider").count(), 1, "{text}");
    assert!(text.contains("provider = \"codex\""), "{text}");
    assert_eq!(
        AccountsConfig::load(&path).expect("reload"),
        load(MIXED).expect("load")
    );
}

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

fn account(provider: Provider, gh: Option<&str>) -> Account {
    Account {
        id: "acct".into(),
        label: "Acct".into(),
        provider,
        config_dir: abs("/home/me/.acct"),
        gh_config_dir: gh.map(abs),
        ssh_agent: false,
    }
}

#[test]
fn codex_account_sets_codex_home_and_never_claude_config_dir() {
    let (_tmp, work, c) = bound(account(Provider::Codex, Some("/home/me/.gh")));
    let env = c.launch_env(&work).expect("env");
    assert_eq!(
        env,
        vec![
            ("CODEX_HOME", abs("/home/me/.acct")),
            ("GH_CONFIG_DIR", abs("/home/me/.gh")),
        ]
    );
    let (_tmp, work, c) = bound(account(Provider::Claude, None));
    assert_eq!(
        c.launch_env(&work).expect("env"),
        vec![("CLAUDE_CONFIG_DIR", abs("/home/me/.acct"))]
    );
}

#[test]
fn relative_codex_home_is_refused() {
    let mut a = account(Provider::Codex, None);
    a.config_dir = ".codex".into();
    assert!(AccountsConfig::default().add_account(a.clone()).is_err());
    assert!(a.launch_env().is_err(), "unvalidated account");
}
