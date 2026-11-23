//! Acceptance: a generated commit message or PR comes from the thread's own account's agent with no tools, MCP or
//! saved session; the prompt carries the thread's title, last prompt and the PR template; a folder bound to another
//! account is refused; Codex answers the same prompts.
#![cfg(unix)]

use skyelab_agent::{commit_prompt, generate_commit_message, generate_pr, pr_prompt};
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

/// `work/` bound to Claude account "work", `other/` to "personal".
fn setup() -> (tempfile::TempDir, PathBuf, Account, Account, AccountsConfig) {
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
    (tmp, work, a, b, config)
}

#[test]
fn a_commit_message_comes_from_the_bound_account_without_tools() {
    let (_tmp, work, a, b, config) = setup();
    let prompt = commit_prompt(
        "Fix the login bug",
        "make the session check stricter",
        "+check()\n",
    );
    let msg =
        generate_commit_message(&fake_claude(), &config, &a, &work, &prompt).expect("message");
    assert_eq!(
        msg,
        "feat(fake): commit from home-work\n\nWritten by the fake agent."
    );

    let log = std::fs::read_to_string(work.join(".fake-title-call")).expect("call logged");
    assert!(
        log.contains(&format!("config_dir={}", a.config_dir.display())),
        "{log}"
    );
    for flag in [
        "arg=[--tools]\narg=[]\n",
        "arg=[--strict-mcp-config]",
        "arg=[--no-session-persistence]",
        "arg=[-p]",
    ] {
        assert!(log.contains(flag), "missing {flag:?} in {log}");
    }
    for context in [
        "Fix the login bug",
        "make the session check stricter",
        "+check()",
    ] {
        assert!(log.contains(context), "missing {context:?} in {log}");
    }

    // The folder is bound to "work": the other account's agent never gets this diff.
    let err = generate_commit_message(&fake_claude(), &config, &b, &work, &prompt)
        .expect_err("wrong account");
    assert!(format!("{err:#}").contains("bound to 'work'"), "{err:#}");
}

#[test]
fn a_pr_fills_the_template() {
    let (_tmp, work, a, _, config) = setup();
    let prompt = pr_prompt(
        "Login",
        "ship it",
        "origin/main",
        "- fix: login",
        "+x\n",
        Some("## Summary\n<!-- what changed -->"),
    );
    let (title, body) = generate_pr(&fake_claude(), &config, &a, &work, &prompt).expect("pr");
    assert_eq!(title, "feat(fake): pull request from home-work");
    assert_eq!(body, "## Summary\nFilled from the template.");
    let log = std::fs::read_to_string(work.join(".fake-title-call")).expect("call logged");
    assert!(
        log.contains("<template>\n## Summary\n<!-- what changed -->\n</template>"),
        "{log}"
    );
    assert!(
        log.contains("- fix: login") && log.contains("into origin/main"),
        "{log}"
    );

    let plain = pr_prompt("Login", "ship it", "main", "- fix: login", "+x\n", None);
    let (_, body) = generate_pr(&fake_claude(), &config, &a, &work, &plain).expect("pr");
    assert_eq!(body, "Fake PR body.");
}

#[test]
fn codex_writes_a_commit_message_under_its_codex_home() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let dir = root.join("project");
    std::fs::create_dir(&dir).expect("mkdir");
    let cx = account(&root, "cx", Provider::Codex);
    let config = AccountsConfig {
        accounts: vec![cx.clone()],
        bindings: vec![Binding::new(dir.clone(), "cx".into())],
    };
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-codex.sh");
    let msg = generate_commit_message(&bin, &config, &cx, &dir, &commit_prompt("t", "p", "+x"))
        .expect("message");
    assert_eq!(msg, "fix(codex): commit from codex");
    let log = std::fs::read_to_string(dir.join(".fake-title-call")).expect("call logged");
    assert!(
        log.contains(&format!("codex_home={}", cx.config_dir.display())),
        "{log}"
    );
    assert!(
        log.contains("arg=[--ephemeral]") && log.contains("arg=[mcp_servers={}]"),
        "{log}"
    );
}
