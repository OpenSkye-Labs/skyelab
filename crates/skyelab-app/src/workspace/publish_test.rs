//! Acceptance: the Changes tab's Commit / Push / Open PR run under the thread's account and its
//! GitHub identity, refuse without one, and the PR link lands on the thread. Fake agent, fake gh.

use super::changes::ChangesView;
use super::changes_model_test::git;
use super::thread::ThreadView;
use super::thread_worktree::OpenChanges;
use super::worktree_thread_test::{Fixture, fixture, open_thread, send, wait_until};
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_core::Binding;

/// A thread row in the fixture's project, and its Changes tab.
pub(super) fn thread_and_changes<'a>(
    f: &Fixture,
    cx: &'a mut TestAppContext,
) -> (
    Entity<ThreadView>,
    Entity<ChangesView>,
    &'a mut VisualTestContext,
) {
    let (ws, thread, cx) = open_thread(f, cx);
    let fake_gh =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-git/tests/fixtures/fake-gh.sh");
    cx.update(|_, cx| cx.set_global(GhBin(fake_gh)));
    send(cx);
    wait_until(cx, "thread row", |cx| {
        thread.read_with(cx, |t, _| t.thread.is_some())
    });
    cx.dispatch_action(OpenChanges);
    cx.run_until_parked();
    let changes = ws
        .read_with(cx, |ws, _| {
            ws.changes_views.last().and_then(|c| c.upgrade())
        })
        .expect("changes tab");
    (thread, changes, cx)
}

pub(super) fn set_input(
    input: &Entity<gpui_component::input::InputState>,
    text: &str,
    cx: &mut VisualTestContext,
) {
    let text = text.to_string();
    input.update_in(cx, |s, window, cx| s.set_value(text, window, cx));
}

/// [`set_input`] for a multi-line field (the commit message).
pub(super) fn set_text(
    input: &Entity<gpui_component::input::TextareaState>,
    text: &str,
    cx: &mut VisualTestContext,
) {
    let text = text.to_string();
    input.update_in(cx, |s, window, cx| s.set_value(text, window, cx));
}

pub(super) fn error(changes: &Entity<ChangesView>, cx: &mut VisualTestContext) -> Option<String> {
    changes.read_with(cx, |c, _| c.publish_error.as_ref().map(|e| e.to_string()))
}

#[gpui::test]
fn push_and_pr_refuse_without_a_github_identity(cx: &mut TestAppContext) {
    let f = fixture(); // "acme" has no gh_config_dir
    let (thread, changes, cx) = thread_and_changes(&f, cx);
    changes.update_in(cx, |c, window, cx| c.push(window, cx));
    wait_until(cx, "push refused", |cx| error(&changes, cx).is_some());
    let e = error(&changes, cx).expect("error");
    assert!(e.contains("no GitHub login"), "{e}");

    set_input(
        &changes.read_with(cx, |c, _| c.pr_title.clone()),
        "Add it",
        cx,
    );
    changes.update_in(cx, |c, window, cx| c.open_pr(window, cx));
    cx.run_until_parked();
    let e = error(&changes, cx).expect("error");
    assert!(e.contains("no GitHub login"), "{e}");
    thread.read_with(cx, |t, _| {
        assert_eq!(t.thread.as_ref().and_then(|t| t.pr_url.clone()), None)
    });
}

#[gpui::test]
fn commit_then_open_pr_stores_the_link_on_the_thread(cx: &mut TestAppContext) {
    let mut f = fixture();
    let gh_dir = f.project.parent().expect("parent").join("gh-acme");
    std::fs::create_dir_all(&gh_dir).expect("mkdir");
    f.config.accounts[0].gh_config_dir = Some(gh_dir.clone());
    let (thread, changes, cx) = thread_and_changes(&f, cx);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&f.project, &["config", k, v]);
    }

    std::fs::write(f.project.join("new.txt"), "hi\n").expect("write");
    // Commit commits only what the tab shows, so it must list new.txt first.
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
    git(&f.project, &["diff", "--quiet", "HEAD"]);
    git(&f.project, &["cat-file", "-e", "HEAD:new.txt"]);

    set_input(
        &changes.read_with(cx, |c, _| c.pr_title.clone()),
        "Add new.txt",
        cx,
    );
    changes.update_in(cx, |c, window, cx| c.open_pr(window, cx));
    wait_until(cx, "pr link", |cx| {
        error(&changes, cx).is_some()
            || thread.read_with(cx, |t, _| {
                t.thread.as_ref().is_some_and(|t| t.pr_url.is_some())
            })
    });
    assert_eq!(error(&changes, cx), None);
    let (id, url) = thread.read_with(cx, |t, _| {
        let t = t.thread.as_ref().expect("row");
        (t.id, t.pr_url.clone().expect("pr url"))
    });
    assert_eq!(url, "https://github.com/acme/demo/pull/1#main");
    let stored = f
        .store
        .lock()
        .expect("lock")
        .thread(id)
        .expect("read")
        .expect("row");
    assert_eq!(stored.pr_url.as_deref(), Some(url.as_str()));
    let env = std::fs::read_to_string(gh_dir.join("fake-gh.env")).expect("gh ran");
    assert!(
        env.lines()
            .any(|l| l == format!("GH_CONFIG_DIR={}", gh_dir.display()))
    );
}

#[gpui::test]
fn a_rebound_folder_never_pushes_the_thread_as_another_account(cx: &mut TestAppContext) {
    let mut f = fixture();
    let gh_dir = f.project.parent().expect("parent").join("gh");
    std::fs::create_dir_all(&gh_dir).expect("mkdir");
    for a in &mut f.config.accounts {
        a.gh_config_dir = Some(gh_dir.clone());
    }
    let (thread, changes, cx) = thread_and_changes(&f, cx);
    // The thread ran as "acme"; now the folder belongs to "other".
    let mut rebound = f.config.clone();
    rebound.remove_binding(&f.project).expect("unbind");
    rebound
        .add_binding(Binding::new(f.project.clone(), "other".into()))
        .expect("rebind");
    thread.update(cx, |t, cx| t.set_config(rebound, cx));

    changes.update_in(cx, |c, window, cx| c.push(window, cx));
    wait_until(cx, "push refused", |cx| error(&changes, cx).is_some());
    let e = error(&changes, cx).expect("error");
    assert!(e.contains("not this thread's account"), "{e}");
    set_input(&changes.read_with(cx, |c, _| c.pr_title.clone()), "x", cx);
    changes.update_in(cx, |c, window, cx| c.open_pr(window, cx));
    cx.run_until_parked();
    assert!(!gh_dir.join("fake-gh.args").exists(), "gh ran");
}

#[gpui::test]
fn the_changes_tab_takes_the_threads_generated_title(cx: &mut TestAppContext) {
    let f = fixture();
    let (thread, changes, cx) = thread_and_changes(&f, cx);
    let id = thread.read_with(cx, |t, _| t.thread.as_ref().map(|t| t.id).expect("row"));
    thread.update(cx, |t, cx| t.retitled(id, "Fix the parser".into(), cx));
    cx.run_until_parked();
    let title = changes.read_with(cx, |c, _| c.title());
    assert_eq!(title.as_ref(), "Changes · Fix the parser");
}

#[gpui::test]
fn coming_back_to_the_window_reads_the_changes_again(cx: &mut TestAppContext) {
    let f = fixture();
    let (_thread, changes, cx) = thread_and_changes(&f, cx);
    let listed = |cx: &mut VisualTestContext| {
        changes.read_with(cx, |c, _| {
            c.changes
                .as_ref()
                .is_some_and(|c| c.files.iter().any(|f| f.path.ends_with("edited-elsewhere.txt")))
        })
    };
    cx.deactivate_window();
    std::fs::write(f.project.join("edited-elsewhere.txt"), "from an editor\n").expect("write");
    cx.run_until_parked();
    assert!(!listed(cx), "nothing told the tab yet");
    cx.update(|window, _| window.activate_window());
    wait_until(cx, "the new file listed", listed);
}
