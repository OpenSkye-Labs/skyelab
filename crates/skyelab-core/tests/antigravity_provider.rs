//! Acceptance: `Provider::Antigravity` isolates accounts via `GEMINI_HOME`, enforces strict folder bindings,
//! refuses stale bindings, and `skyelab_core::env::agent_allowed` strips inherited Antigravity/Gemini credentials.

use skyelab_core::env::agent_allowed;
use skyelab_core::onboarding::{default_config_dir, find_accounts};
use skyelab_core::thread_defaults::ThreadField;
use skyelab_core::{Account, AccountsConfig, Binding, Provider, ThreadSettings};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

fn abs(p: &str) -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(format!("C:{}", p.replace('/', "\\")))
    }
    #[cfg(not(windows))]
    {
        PathBuf::from(p)
    }
}

#[test]
fn antigravity_provider_uses_gemini_home_and_enforces_bindings() {
    assert_eq!(Provider::Antigravity.label(), "Antigravity");
    assert_eq!(Provider::Antigravity.home_var(), "GEMINI_HOME");
    assert!(Provider::ALL.contains(&Provider::Antigravity));

    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let bound = root.join("work");
    let unbound = root.join("other");
    std::fs::create_dir_all(&bound).expect("mkdir bound");
    std::fs::create_dir_all(&unbound).expect("mkdir unbound");

    let gemini_home = root.join(".antigravity-work");
    let mut cfg = AccountsConfig::default();
    cfg.add_account(Account {
        id: "agy-work".into(),
        label: "Antigravity Work".into(),
        provider: Provider::Antigravity,
        config_dir: gemini_home.clone(),
        gh_config_dir: None,
        ssh_agent: false,
    })
    .expect("add account");
    cfg.add_binding(Binding::new(bound.clone(), "agy-work".into()))
        .expect("bind");

    let saved_path = root.join("accounts.toml");
    cfg.save(&saved_path).expect("save");
    let loaded = AccountsConfig::load(&saved_path).expect("load");

    let resolved = loaded.resolve(&bound).expect("bound folder resolves");
    assert_eq!(resolved.provider, Provider::Antigravity);
    assert_eq!(
        loaded.launch_env(&bound).expect("launch_env"),
        vec![("GEMINI_HOME", gemini_home.clone())]
    );

    // Unbound folder never falls back to a default account.
    assert!(loaded.resolve(&unbound).is_none());
    assert!(loaded.launch_env(&unbound).is_err());

    // Stale binding (unconfirmed TOML without `resolved`) is refused.
    let stale_toml = format!(
        "[[account]]\nid = \"agy\"\nlabel = \"A\"\nprovider = \"antigravity\"\nconfig_dir = {:?}\n\
         [[binding]]\npath = {:?}\naccount = \"agy\"\n",
        abs("/home/me/.antigravity-a").to_string_lossy(),
        bound.to_string_lossy()
    );
    let stale_path = root.join("stale.toml");
    std::fs::write(&stale_path, stale_toml).expect("write stale");
    let stale = AccountsConfig::load(&stale_path).expect("load stale");
    assert!(
        stale.resolve(&bound).is_none(),
        "stale binding must be refused"
    );
}

#[test]
fn env_allowlist_strips_gemini_and_google_credentials_on_unix_and_windows() {
    for windows in [false, true] {
        for var in [
            "GEMINI_HOME",
            "gemini_home",
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "GOOGLE_CLOUD_PROJECT",
            "AGY_ACP_FORCE_FILE_STORAGE",
        ] {
            assert!(
                !agent_allowed(OsStr::new(var), windows, false),
                "{var} must be stripped (windows={windows})"
            );
        }
    }
}

#[test]
fn onboarding_and_thread_defaults_support_antigravity() {
    let home = tempfile::tempdir().expect("tempdir");
    let h = home.path();
    let default_dir = default_config_dir(h, Provider::Antigravity, "work").expect("default dir");
    assert_eq!(default_dir, h.join(".antigravity-work"));
    std::fs::create_dir_all(default_dir.join("antigravity-acp")).expect("mkdir marker");

    let found = find_accounts(h, &AccountsConfig::default());
    assert!(
        found
            .iter()
            .any(|f| f.provider == Provider::Antigravity && f.config_dir == default_dir),
        "{found:?}"
    );

    let mut ts = ThreadSettings::default();
    ts.set_field(ThreadField::AntigravityModel, Some("gemini-2.5-pro".into()));
    ts.set_field(ThreadField::AntigravityEffort, Some("high".into()));
    assert_eq!(
        ts.model_for(Provider::Antigravity),
        (Some("gemini-2.5-pro".into()), Some("high".into()))
    );
    let proj = Path::new("/tmp/p");
    ts.set_project_field(proj, ThreadField::AntigravityEffort, Some("low".into()));
    assert_eq!(
        ts.resolve_model(proj, Provider::Antigravity, (None, None), (None, None)),
        (Some("gemini-2.5-pro".into()), Some("low".into()))
    );
}
