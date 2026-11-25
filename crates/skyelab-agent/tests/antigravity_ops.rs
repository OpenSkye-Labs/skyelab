//! Acceptance: Antigravity auxiliary operations (`login_status`, `list_antigravity_models`, `generate_title`,
//! `usage_limits`, `import::list`, `import::project_folders`) honor `GEMINI_HOME` isolation and safe tool refusal.
#![cfg(unix)]

use skyelab_agent::{
    Limits, Login, generate_title, import, list_antigravity_models, login_status, title_prompt,
    usage_limits,
};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::{Path, PathBuf};

fn fake_antigravity() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-antigravity.sh")
}

fn account(id: &str, dir: &Path) -> Account {
    Account {
        id: id.into(),
        label: id.into(),
        provider: Provider::Antigravity,
        config_dir: dir.to_owned(),
        gh_config_dir: None,
        ssh_agent: false,
    }
}

#[test]
fn login_status_models_title_usage_and_import_for_antigravity() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let proj = root.join("proj");
    std::fs::create_dir_all(&proj).expect("mkdir proj");
    let (home_a, home_b) = (root.join("home-a"), root.join("home-b"));
    std::fs::create_dir_all(&home_a).expect("mkdir a");
    std::fs::create_dir_all(&home_b).expect("mkdir b");
    std::fs::write(home_a.join("fake-login"), "a@example.com\n").expect("sign a in");

    let acct_a = account("agy-a", &home_a);
    let acct_b = account("agy-b", &home_b);
    let missing = account("agy-none", &root.join("missing"));
    let bin = fake_antigravity();

    // login_status isolates per GEMINI_HOME.
    assert!(matches!(
        login_status(&bin, &acct_a).expect("a status"),
        Login::SignedIn(_)
    ));
    assert_eq!(
        login_status(&bin, &acct_b).expect("b status"),
        Login::SignedOut
    );
    assert_eq!(
        login_status(&bin, &missing).expect("missing dir"),
        Login::SignedOut
    );

    // list_antigravity_models enforces folder binding and returns models + thought_level efforts.
    let config = AccountsConfig {
        accounts: vec![acct_a.clone()],
        bindings: vec![Binding::new(proj.clone(), "agy-a".into())],
    };
    let models = list_antigravity_models(&bin, &config, &acct_a, &proj).expect("models");
    assert_eq!(models.len(), 3);
    assert_eq!(models[0].id, "gemini-3-flash");
    assert!(models[0].is_default);
    assert_eq!(models[0].efforts, ["low", "medium", "high"]);
    assert_eq!(models[0].default_effort.as_deref(), Some("medium"));
    assert!(
        models.iter().any(|m| m.id == "claude-sonnet-4-5"),
        "Antigravity lists non-Gemini models too"
    );
    // Unbound folder refused.
    assert!(list_antigravity_models(&bin, &config, &acct_a, &root).is_err());

    // generate_title uses bounded ACP one_shot.
    let prompt = title_prompt("Fix login", "Fixed it.");
    let title = generate_title(&bin, &config, &acct_a, &proj, &prompt).expect("title");
    assert_eq!(title, "Antigravity fake title");

    // one_shot must auto-cancel any permission request from the agent (no tools).
    let safe = skyelab_agent::one_shot(&bin, &config, &acct_a, &proj, "one_shot_ask", "title")
        .expect("one_shot");
    assert_eq!(safe.trim(), "Safe title");
    let perm_reply =
        std::fs::read_to_string(home_a.join("fake-oneshot-perm")).expect("perm reply recorded");
    assert!(
        perm_reply.contains("\"cancelled\""),
        "one_shot must cancel tool permissions, got: {perm_reply}"
    );

    // usage_limits returns Unavailable without error; import returns empty.
    assert_eq!(
        usage_limits(&bin, &acct_a).expect("usage"),
        Limits::Unavailable
    );
    assert!(import::list(Provider::Antigravity, &home_a, &proj).is_empty());
    assert!(import::project_folders(Provider::Antigravity, &home_a).is_empty());
}
