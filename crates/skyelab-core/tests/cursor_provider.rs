//! Acceptance: `Provider::Cursor` has `home_var() == "CURSOR_CONFIG_DIR"`, sets only `CURSOR_CONFIG_DIR` (plus
//! optional `GH_CONFIG_DIR`) in `launch_env`, enforces strict folder bindings (unbound and stale bindings return
//! `None`), strips inherited Cursor env vars in `skyelab_core::env`, and integrates with onboarding and thread
//! defaults.

use skyelab_core::env::{agent_vars, terminal_vars};
use skyelab_core::onboarding::{default_config_dir, find_accounts};
use skyelab_core::thread_defaults::ThreadField;
use skyelab_core::{Account, AccountsConfig, Binding, Provider, ThreadSettings};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::PathBuf;

const MIXED: &str = r#"
[[account]]
id = "work"
label = "Work"
config_dir = "/home/me/.claude-work"

[[account]]
id = "cur"
label = "Cursor Work"
provider = "cursor"
config_dir = "/home/me/.cursor-work"
gh_config_dir = "/home/me/.config/gh-cur"
"#;

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
fn cursor_provider_metadata_and_round_trip() {
    assert!(Provider::ALL.contains(&Provider::Cursor));
    assert_eq!(Provider::Cursor.label(), "Cursor");
    assert_eq!(Provider::Cursor.home_var(), "CURSOR_CONFIG_DIR");

    let c = load(MIXED).expect("load");
    assert_eq!(c.accounts[0].provider, Provider::Claude);
    assert_eq!(c.accounts[1].provider, Provider::Cursor);

    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("accounts.toml");
    c.save(&path).expect("save");
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(text.contains("provider = \"cursor\""), "{text}");
    assert_eq!(AccountsConfig::load(&path).expect("reload"), c);
}

#[test]
fn cursor_launch_env_and_strict_binding_invariant() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let bound_dir = root.join("proj");
    let unbound_dir = root.join("other");
    std::fs::create_dir(&bound_dir).expect("mkdir");
    std::fs::create_dir(&unbound_dir).expect("mkdir");

    let account = Account {
        id: "cur".into(),
        label: "Cursor".into(),
        provider: Provider::Cursor,
        config_dir: abs("/home/me/.cursor-work"),
        gh_config_dir: Some(abs("/home/me/.config/gh-cur")),
        ssh_agent: false,
    };
    let mut c = AccountsConfig::default();
    c.add_account(account).expect("add");
    c.add_binding(Binding::new(bound_dir.clone(), "cur".into()))
        .expect("bind");

    assert_eq!(
        c.launch_env(&bound_dir).expect("env"),
        vec![
            ("CURSOR_CONFIG_DIR", abs("/home/me/.cursor-work")),
            ("GH_CONFIG_DIR", abs("/home/me/.config/gh-cur")),
        ]
    );
    assert!(
        c.resolve(&unbound_dir).is_none(),
        "unbound folders never fall back to a Cursor account"
    );
    assert!(c.launch_env(&unbound_dir).is_err());

    // Relative CURSOR_CONFIG_DIR is refused.
    let bad = Account {
        id: "bad".into(),
        label: "Bad".into(),
        provider: Provider::Cursor,
        config_dir: ".cursor".into(),
        gh_config_dir: None,
        ssh_agent: false,
    };
    assert!(AccountsConfig::default().add_account(bad.clone()).is_err());
    assert!(bad.launch_env().is_err());

    // Unconfirmed / stale binding resolves to None.
    let stale_toml = format!(
        "[[account]]\nid = \"cur\"\nlabel = \"Cursor\"\nprovider = \"cursor\"\nconfig_dir = \"{}\"\n\n[[binding]]\npath = \"{}\"\naccount = \"cur\"\n",
        root.join(".cursor-work")
            .display()
            .to_string()
            .replace('\\', "/"),
        bound_dir.display().to_string().replace('\\', "/"),
    );
    let stale_cfg = load(&stale_toml).expect("load stale");
    assert!(
        stale_cfg.resolve(&bound_dir).is_none(),
        "stale binding without resolved snapshot must refuse"
    );
}

#[test]
fn env_allowlist_strips_cursor_credentials_on_unix_and_windows() {
    let parent: Vec<(OsString, OsString)> = [
        ("CURSOR_CONFIG_DIR", "/parent/cursor"),
        ("CURSOR_API_KEY", "cur_secret_key"),
        ("CURSOR_AUTH_TOKEN", "cur_secret_token"),
        ("CURSOR_BACKEND_URL", "https://evil.invalid"),
        ("cursor_api_key", "cur_secret_lower"),
        ("HOME", "/home/me"),
        ("TZ", "UTC"),
    ]
    .into_iter()
    .map(|(k, v)| (OsString::from(k), OsString::from(v)))
    .collect();

    for windows in [false, true] {
        for vars in [
            agent_vars(parent.clone(), windows, false),
            terminal_vars(parent.clone(), windows),
        ] {
            let names: BTreeSet<String> = vars
                .into_iter()
                .map(|(k, _)| k.to_string_lossy().to_ascii_uppercase())
                .collect();
            for forbidden in [
                "CURSOR_CONFIG_DIR",
                "CURSOR_API_KEY",
                "CURSOR_AUTH_TOKEN",
                "CURSOR_BACKEND_URL",
            ] {
                assert!(
                    !names.contains(forbidden),
                    "{forbidden} leaked (windows={windows}): {names:?}"
                );
            }
        }
    }
}

#[test]
fn onboarding_discovers_cursor_config_dirs_and_defaults_resolve() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = std::fs::canonicalize(tmp.path()).expect("canonical");
    assert_eq!(
        default_config_dir(&home, Provider::Cursor, "work"),
        Some(home.join(".cursor-work"))
    );
    std::fs::create_dir_all(home.join(".cursor/chats")).expect("mkdir");
    std::fs::create_dir_all(home.join(".cursor-work")).expect("mkdir");
    std::fs::write(home.join(".cursor-work/cli-config.json"), "{}").expect("write");

    let found = find_accounts(&home, &AccountsConfig::default());
    let pairs: Vec<(Provider, &str)> = found.iter().map(|f| (f.provider, f.id.as_str())).collect();
    assert_eq!(
        pairs,
        vec![
            (Provider::Cursor, "cursor"),
            (Provider::Cursor, "cursor-work")
        ]
    );

    let mut settings = ThreadSettings::default();
    settings.set_field(ThreadField::CursorModel, Some("composer-1".into()));
    assert_eq!(
        settings.model_for(Provider::Cursor),
        (Some("composer-1".into()), None)
    );
    let proj = home.join("proj");
    settings.set_project_field(&proj, ThreadField::CursorModel, Some("auto".into()));
    assert_eq!(
        settings.resolve_model(&proj, Provider::Cursor, (None, None), (None, None)),
        (Some("auto".into()), None)
    );
}
