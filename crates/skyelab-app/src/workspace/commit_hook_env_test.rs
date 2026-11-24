//! The repo's commit hooks run with the bound account's git credential binding, the one its agents get: every
//! configured helper (a keychain among them) is reset and only the account's gh answers, ssh is refused unless the
//! account allows it, and git never prompts.

use super::changes_model_test::git;
use super::publish_test::{error, set_text, thread_and_changes};
use super::worktree_thread_test::{fixture, wait_until};
use gpui::TestAppContext;
use std::os::unix::fs::PermissionsExt as _;

const KEYCHAIN: &str = "!echo password=from-the-keychain";

#[gpui::test]
fn commit_hooks_get_the_accounts_git_credential_binding(cx: &mut TestAppContext) {
    let mut f = fixture();
    let root = f.project.parent().expect("parent").to_path_buf();
    let gh_dir = root.join("gh-acme");
    std::fs::create_dir_all(&gh_dir).expect("mkdir");
    f.config.accounts[0].gh_config_dir = Some(gh_dir);
    let (_thread, changes, cx) = thread_and_changes(&f, cx);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
        ("credential.helper", KEYCHAIN),
    ] {
        git(&f.project, &["config", k, v]);
    }
    let bare = root.join("bare.git");
    git(
        &root,
        &["init", "-q", "--bare", bare.to_str().expect("utf-8")],
    );
    let report = root.join("hook-report.txt");
    let hooks = f.project.join(".git/hooks");
    let write_hook = |name: &str, body: String| {
        let path = hooks.join(name);
        std::fs::write(&path, body).expect("hook");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    };
    write_hook(
        "pre-commit",
        format!(
            "#!/bin/sh\n{{ git config --get-all credential.helper; echo \"ssh=$(git config core.sshCommand)\"; \
             echo \"prompt=$GIT_TERMINAL_PROMPT\"; }} > '{}'\n",
            report.display()
        ),
    );
    write_hook(
        "post-commit",
        format!(
            "#!/bin/sh\ngit push -q '{}' HEAD:refs/heads/from-hook\n",
            bare.display()
        ),
    );

    std::fs::write(f.project.join("new.txt"), "hi\n").expect("write");
    changes.update(cx, |c, cx| c.refresh(cx));
    wait_until(cx, "new.txt listed", |cx| {
        changes.read_with(cx, |c, _| {
            c.changes
                .as_ref()
                .is_some_and(|ch| ch.files.iter().any(|f| f.path.ends_with("new.txt")))
        })
    });
    set_text(
        &changes.read_with(cx, |c, _| c.commit_message.clone()),
        "Add new.txt",
        cx,
    );
    changes.update_in(cx, |c, window, cx| c.commit(window, cx));
    wait_until(cx, "committed", |cx| {
        changes.read_with(cx, |c, _| {
            c.publish_error.is_some() || c.publish_note.is_some()
        })
    });
    assert_eq!(error(&changes, cx), None);

    let report = std::fs::read_to_string(&report).expect("the hook ran");
    let lines: Vec<&str> = report.lines().collect();
    let helpers: Vec<&str> = lines
        .iter()
        .copied()
        .take_while(|l| !l.starts_with("ssh="))
        .collect();
    let keychain = helpers
        .iter()
        .position(|h| *h == KEYCHAIN)
        .expect("the repo's helper is configured");
    let reset = helpers
        .iter()
        .rposition(|h| h.is_empty())
        .expect("the helper list is reset");
    assert!(
        reset > keychain,
        "the reset comes after the keychain: {helpers:?}"
    );
    assert_eq!(
        helpers.last(),
        Some(&"!gh auth git-credential"),
        "{helpers:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("ssh=") && l.contains("ssh is off")),
        "{report}"
    );
    assert!(lines.contains(&"prompt=0"), "{report}");
    git(&bare, &["rev-parse", "--verify", "refs/heads/from-hook"]);
}

#[test]
fn the_note_says_why_hooks_were_skipped() {
    use super::changes_publish::hooks_note;
    assert_eq!(hooks_note("Pushed".into(), Some(true)), "Pushed");
    assert_eq!(
        hooks_note("Pushed".into(), Some(false)),
        "Pushed. Hooks skipped: this repo is shared with another account"
    );
    assert_eq!(
        hooks_note("Pushed".into(), None),
        "Pushed. Hooks skipped: couldn't confirm this repo is single-account"
    );
}
