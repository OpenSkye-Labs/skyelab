//! Acceptance: the Changes tab's sparkles fill the commit message, or the PR title and body (from the repo's PR
//! template), from the thread's own account's agent; nothing is committed; an edit made meanwhile is kept; a folder
//! bound elsewhere is refused; Settings → Threads hides and refuses it.

use super::changes::ChangesView;
use super::changes_generate::{Gen, generate};
use super::changes_model_test::git;
use super::publish_test::{error, set_text, thread_and_changes};
use super::thread_extras::prefs::ThreadPrefs;
use super::thread_extras::title::Bins;
use super::worktree_thread_test::{fixture, wait_until};
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_core::{Binding, ThreadSettings};

fn value(
    field: &Entity<gpui_component::input::TextareaState>,
    cx: &mut VisualTestContext,
) -> String {
    field.read_with(cx, |s, _| s.value().to_string())
}

fn run(changes: &Entity<ChangesView>, what: Gen, cx: &mut VisualTestContext) {
    changes.update_in(cx, |c, window, cx| c.generate(what, window, cx));
}

fn wait_generated(changes: &Entity<ChangesView>, cx: &mut VisualTestContext) {
    wait_until(cx, "generated", |cx| {
        changes.read_with(cx, |c, _| c.generating.is_none())
    });
}

#[gpui::test]
fn the_sparkle_fills_the_commit_message_and_commits_nothing(cx: &mut TestAppContext) {
    let f = fixture();
    let (_thread, changes, cx) = thread_and_changes(&f, cx);
    std::fs::write(f.project.join("README.md"), "# demo\nmore docs\n").expect("write");
    let head = || skyelab_git::rev_parse(&f.project, "HEAD").expect("head");
    let before = head();

    run(&changes, Gen::Commit, cx);
    wait_generated(&changes, cx);
    assert_eq!(error(&changes, cx), None);
    let field = changes.read_with(cx, |c, _| c.commit_message.clone());
    assert_eq!(
        value(&field, cx),
        "feat(fake): commit from .claude-acme\n\nWritten by the fake agent."
    );
    assert_eq!(head(), before, "nothing committed");
    let log = std::fs::read_to_string(f.project.join(".fake-title-call")).expect("one call");
    assert!(log.contains("+more docs"), "the diff: {log}");
    assert!(log.contains("fix it"), "the thread's last prompt: {log}");
    assert!(
        log.contains(&format!("config_dir={}", f.account_dir.display())),
        "{log}"
    );

    // A field edited while the agent writes keeps the edit.
    set_text(&field, "", cx);
    run(&changes, Gen::Commit, cx);
    set_text(&field, "my own words", cx);
    wait_generated(&changes, cx);
    assert_eq!(value(&field, cx), "my own words");
    assert!(
        changes.read_with(cx, |c, _| c.publish_note.is_some()),
        "says why it left the field"
    );
}

#[gpui::test]
fn the_pr_sparkle_fills_the_template(cx: &mut TestAppContext) {
    let f = fixture();
    std::fs::create_dir(f.project.join(".github")).expect("mkdir");
    std::fs::write(
        f.project.join(".github/pull_request_template.md"),
        "## Summary\n",
    )
    .expect("write");
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&f.project, &["config", k, v]);
    }
    git(&f.project, &["add", "."]);
    git(&f.project, &["commit", "-q", "-m", "template"]);
    git(&f.project, &["switch", "-q", "-c", "feature"]);
    std::fs::write(f.project.join("README.md"), "# demo\nfeature\n").expect("write");
    git(&f.project, &["commit", "-q", "-am", "feat: a feature"]);

    let (_thread, changes, cx) = thread_and_changes(&f, cx);
    run(&changes, Gen::Pr, cx);
    wait_generated(&changes, cx);
    assert_eq!(error(&changes, cx), None);
    let (title, body) = changes.read_with(cx, |c, cx| {
        (
            c.pr_title.read(cx).value().to_string(),
            c.pr_body.read(cx).value().to_string(),
        )
    });
    assert_eq!(title, "feat(fake): pull request from .claude-acme");
    assert_eq!(body, "## Summary\nFilled from the template.");
    let log = std::fs::read_to_string(f.project.join(".fake-title-call")).expect("one call");
    assert!(
        log.contains("- feat: a feature") && log.contains("<template>"),
        "{log}"
    );
}

#[gpui::test]
fn a_folder_bound_elsewhere_is_refused(cx: &mut TestAppContext) {
    let f = fixture();
    std::fs::write(f.project.join("README.md"), "# changed\n").expect("write");
    let mut moved = f.config.clone();
    moved.bindings = vec![Binding::new(f.project.clone(), "other".into())];
    let bins = cx.update(|cx| Bins::new(PathBuf::from("/nonexistent/claude"), cx));
    let err = generate(
        Gen::Commit,
        &f.store,
        &moved,
        &bins,
        (1, "acme"),
        &f.project,
        &[],
    )
    .expect_err("refused");
    assert!(
        format!("{err:#}").contains("isn't bound to acme"),
        "{err:#}"
    );
    assert!(!f.project.join(".fake-title-call").exists());
}

#[gpui::test]
fn turned_off_hides_the_sparkles_and_refuses(cx: &mut TestAppContext) {
    let f = fixture();
    let (_thread, changes, cx) = thread_and_changes(&f, cx);
    let shown = |cx: &mut VisualTestContext| {
        changes.update(cx, |c, cx| c.generate_button(Gen::Commit, cx).is_some())
    };
    assert!(shown(cx), "on by default");
    cx.update(|_, cx| {
        cx.set_global(ThreadPrefs {
            path: None,
            settings: ThreadSettings {
                generate_git_text: false,
                ..Default::default()
            },
            diff: Default::default(),
            terminal: Default::default(),
            visual: Default::default(),
        })
    });
    assert!(!shown(cx));
    run(&changes, Gen::Commit, cx);
    let e = error(&changes, cx).expect("refused");
    assert!(e.contains("off in Settings"), "{e}");
    assert!(!f.project.join(".fake-title-call").exists());
}
