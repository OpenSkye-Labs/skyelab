//! Acceptance: a Claude title call runs under the thread's own account with no tools and no saved session, and its
//! prompt carries only the two inputs.
#![cfg(unix)]

use skyelab_agent::{generate_title, title_prompt};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::{Path, PathBuf};

fn account(root: &Path, id: &str, provider: Provider) -> Account {
    Account {
        id: id.into(),
        label: id.into(),
        provider,
        config_dir: root.join(format!("home-{id}")),
        gh_config_dir: None,
        ssh_agent: false,
    }
}

fn fake_claude() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-app/tests/fixtures/fake-claude.sh")
}

#[test]
fn claude_title_runs_bound_without_tools_or_session() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let (work, other) = (root.join("work"), root.join("other"));
    std::fs::create_dir(&work).expect("mkdir");
    std::fs::create_dir(&other).expect("mkdir");
    let (a, b) = (
        account(&root, "work", Provider::Claude),
        account(&root, "personal", Provider::Claude),
    );
    let config = AccountsConfig {
        accounts: vec![a.clone(), b.clone()],
        bindings: vec![
            Binding::new(work.clone(), "work".into()),
            Binding::new(other, "personal".into()),
        ],
    };
    let prompt = title_prompt("fix the login bug", "I fixed the session check.");
    let title = generate_title(&fake_claude(), &config, &a, &work, &prompt).expect("title");
    assert_eq!(title, "Fake title from home-work");

    let log = std::fs::read_to_string(work.join(".fake-title-call")).expect("call logged");
    assert!(
        log.contains(&format!("config_dir={}", a.config_dir.display())),
        "{log}"
    );
    for flag in [
        "arg=[--tools]\narg=[]\n",
        "arg=[--no-session-persistence]",
        "arg=[--model]\narg=[haiku]",
        "arg=[-p]",
    ] {
        assert!(log.contains(flag), "missing {flag:?} in {log}");
    }
    assert!(log.contains("fix the login bug") && log.contains("I fixed the session check."));

    // The other account's folder never gets this account's call.
    let err =
        generate_title(&fake_claude(), &config, &b, &work, &prompt).expect_err("wrong account");
    assert!(format!("{err:#}").contains("bound to 'work'"), "{err:#}");
}
