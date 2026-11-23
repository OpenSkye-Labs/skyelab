//! Friendly model names, and the account's own default model read from its config dir (the model field only).

use skyelab_agent::{account_default_model, model_label};
use skyelab_core::{Account, Provider};

#[test]
fn model_label_names_models_the_way_people_say_them() {
    for (id, label) in [
        ("claude-opus-5-5", "Opus 5.5"),
        ("claude-sonnet-4-5-20250929", "Sonnet 4.5"),
        ("claude-opus-4-1[1m]", "Opus 4.1 (1M)"),
        ("claude-haiku-4-5", "Haiku 4.5"),
        ("claude-fable-5-2", "Fable 5.2"),
        ("claude-3-5-sonnet-20241022", "Sonnet 3.5"),
        ("opus", "Opus"),
        ("sonnet[1m]", "Sonnet (1M)"),
        ("gpt-5.5", "GPT-5.5"),
        ("gpt-5.3-codex", "GPT-5.3-Codex"),
        ("some-model", "some-model"),
    ] {
        assert_eq!(model_label(id), label, "{id}");
    }
}

fn account(provider: Provider, dir: &std::path::Path) -> Account {
    Account {
        id: "work".into(),
        label: "work".into(),
        provider,
        config_dir: dir.to_path_buf(),
        gh_config_dir: None,
        ssh_agent: false,
    }
}

#[test]
fn account_default_takes_only_settings_model() {
    let dir = tempfile::tempdir().expect("tempdir");
    let a = account(Provider::Claude, dir.path());
    assert_eq!(account_default_model(&a), None, "no settings.json");
    std::fs::write(
        dir.path().join(".credentials.json"),
        r#"{"claudeAiOauth":{"accessToken":"sk-secret","model":"secret-model"}}"#,
    )
    .expect("write");
    assert_eq!(
        account_default_model(&a),
        None,
        "credentials are never read"
    );
    std::fs::write(
        dir.path().join("settings.json"),
        r#"{"apiKeyHelper":"echo sk-secret","env":{"ANTHROPIC_MODEL":"haiku","TOKEN":"sk-secret"},"model":"opus"}"#,
    )
    .expect("write");
    let before = std::fs::read(dir.path().join("settings.json")).expect("read");
    assert_eq!(account_default_model(&a).as_deref(), Some("opus"));
    assert_eq!(
        std::fs::read(dir.path().join("settings.json")).expect("read"),
        before,
        "never written"
    );
    for bad in [
        r#"{"model":7}"#,
        r#"{"model":"--dangerously-skip-permissions"}"#,
        "not json",
        r#"{"env":{"model":"x"}}"#,
    ] {
        std::fs::write(dir.path().join("settings.json"), bad).expect("write");
        assert_eq!(account_default_model(&a), None, "{bad}");
    }
}

#[test]
fn account_default_for_codex_reads_config_toml_model() {
    let dir = tempfile::tempdir().expect("tempdir");
    let a = account(Provider::Codex, dir.path());
    std::fs::write(dir.path().join("auth.json"), r#"{"model":"secret"}"#).expect("write");
    assert_eq!(account_default_model(&a), None);
    std::fs::write(
        dir.path().join("config.toml"),
        "model = \"gpt-5.5\"\n[profiles.x]\nmodel = \"other\"\n",
    )
    .expect("write");
    assert_eq!(account_default_model(&a).as_deref(), Some("gpt-5.5"));
    std::fs::write(
        dir.path().join("config.toml"),
        "[profiles.x]\nmodel = \"other\"\n",
    )
    .expect("write");
    assert_eq!(
        account_default_model(&a),
        None,
        "a profile's model is not the default"
    );
}
