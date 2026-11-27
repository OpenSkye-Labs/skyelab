//! Follow-ups to worktree threads: ignored files on discard, the diff base, a Changes tab shared
//! by threads, and the cleanup when a new thread's row can't be saved.

use super::changes_model_test::{git, repo};
use super::worktree::{self, Discard, discard};
use std::path::{Path, PathBuf};

fn add_worktree(root: &Path, name: &str) -> PathBuf {
    let wt = root.join(".skyelab/worktrees").join(name);
    let branch = format!("skyelab/{name}");
    let plain = skyelab_core::files::without_verbatim(&wt);
    let dest = plain.to_str().expect("utf-8");
    git(
        root,
        &["worktree", "add", "-q", "-b", &branch, dest, "HEAD"],
    );
    wt
}

/// A repo that ignores `.env` and `node_modules/`, with both present in a new worktree.
fn ignored_worktree() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let (tmp, root) = repo(&[(".gitignore", b".env\nnode_modules/\n")]);
    let wt = add_worktree(&root, "w");
    std::fs::write(wt.join(".env"), "SECRET=1\n").expect("env");
    std::fs::create_dir_all(wt.join("node_modules/x")).expect("dir");
    std::fs::write(wt.join("node_modules/x/i.js"), "x\n").expect("js");
    (tmp, root, wt)
}

#[test]
fn remove_worktree_refuses_ignored_files_without_force() {
    let (_tmp, root, wt) = ignored_worktree();
    let err = skyelab_git::remove_worktree(&root, &wt, false)
        .expect_err("ignored files would be deleted without asking");
    assert!(matches!(err, skyelab_git::Error::Dirty(_)), "{err:?}");
    assert!(wt.join(".env").exists());
}

#[test]
fn discard_asks_before_deleting_ignored_files() {
    let (_tmp, _root, wt) = ignored_worktree();
    let Discard::NeedsConfirm(shown) = discard(&wt, None).expect("discard") else {
        panic!("the ignored .env was deleted without asking");
    };
    assert!(wt.join(".env").exists());
    // One entry for the ignored folder, not every file in it.
    let mut ignored = shown.ignored.clone();
    ignored.sort();
    assert_eq!(
        ignored,
        [PathBuf::from(".env"), PathBuf::from("node_modules/")]
    );
    let msg = worktree::discard_message(&shown);
    assert!(
        msg.contains(".env") && msg.contains("node_modules/") && msg.contains("ignored"),
        "{msg}"
    );

    assert!(matches!(
        discard(&wt, Some(&shown)).expect("discard"),
        Discard::Removed
    ));
    assert!(!wt.exists());
}

#[test]
fn failed_row_save_removes_the_new_worktree() {
    let f = super::worktree_thread_test::fixture();
    let created =
        worktree::create(&f.config, &f.project, "acme", "doomed", None, None).expect("create");
    let path = created.path.clone();
    assert!(path.exists());
    // No project 999 in the store, so the row insert fails on its foreign key.
    let new = skyelab_store::NewThread {
        project_id: 999,
        title: "doomed",
        cwd: &path,
        account_id: "acme",
        status: "starting",
    };
    let err = worktree::save(&f.store, &f.project, &new, Some(created), None, None)
        .expect_err("the row can't be saved");
    assert!(!path.exists(), "worktree left behind after: {err:#}");
}

#[cfg(unix)] // only the fake-claude.sh test reads it
fn head(dir: &Path) -> String {
    let out = skyelab_core::process::output(
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["rev-parse", "HEAD"]),
        None,
    )
    .expect("git runs");
    String::from_utf8(out.stdout)
        .expect("utf-8")
        .trim()
        .to_owned()
}

#[cfg(unix)] // the fake agent is a shell script
#[gpui::test]
fn changes_show_the_agents_commits_against_the_worktree_base(cx: &mut gpui::TestAppContext) {
    use super::thread_worktree::{OpenChanges, ToggleWorktree};
    use super::worktree_thread_test::{fixture, open_thread, send, wait_until};
    let f = fixture();
    // The fake agent commits a file when it starts in a checkout holding this marker.
    std::fs::write(f.project.join(".fake-agent-commits"), "").expect("marker");
    git(&f.project, &["add", "."]);
    git(&f.project, &["commit", "-q", "-m", "marker"]);
    let base = head(&f.project);

    let (ws, thread, cx) = open_thread(&f, cx);
    cx.dispatch_action(ToggleWorktree);
    send(cx);
    wait_until(cx, "thread row", |cx| {
        thread.read_with(cx, |t, _| t.thread.is_some() || t.error.is_some())
    });
    let row = thread.read_with(cx, |t, _| t.thread.clone().expect("row"));
    let wt = row.worktree_path.clone().expect("worktree");
    assert_eq!(row.worktree_base.as_deref(), Some(base.as_str()));
    let stored = f.store.lock().expect("lock").thread(row.id).expect("read");
    assert_eq!(stored.expect("row").worktree_base, Some(base.clone()));
    wait_until(cx, "the agent's commit", |_| head(&wt) != base);
    std::fs::write(wt.join("README.md"), "# edited, not committed\n").expect("edit");

    cx.dispatch_action(OpenChanges);
    let view = ws
        .read_with(&*cx, |ws, _| {
            ws.changes_views.last().and_then(|c| c.upgrade())
        })
        .expect("changes tab");
    wait_until(cx, "the diff", |cx| {
        view.read_with(cx, |c, _| c.changes.is_some() || c.error.is_some())
    });
    view.read_with(cx, |c, _| {
        assert_eq!(c.error, None);
        let files: Vec<_> = c
            .changes
            .as_ref()
            .expect("loaded")
            .files
            .iter()
            .map(|f| f.path.clone())
            .collect();
        assert_eq!(
            files,
            [
                PathBuf::from("README.md"),
                PathBuf::from("agent-commit.txt")
            ],
            "committed and uncommitted changes both show"
        );
    });
}

#[gpui::test]
fn changes_tab_refreshes_for_every_thread_of_its_folder(cx: &mut gpui::TestAppContext) {
    use super::thread::ThreadViewEvent;
    use super::worktree_thread_test::{fixture, open_thread, wait_until};
    let f = fixture();
    let (ws, first, cx) = open_thread(&f, cx);
    cx.dispatch_action(super::NewThread);
    cx.run_until_parked();
    let second = ws
        .read_with(&*cx, |ws, _| ws.open_threads.last().cloned())
        .expect("second thread tab");
    assert_ne!(first, second);
    // Both threads run in the project folder, so they share one Changes tab.
    for t in [&first, &second] {
        t.update(cx, |_, cx| cx.emit(ThreadViewEvent::OpenChanges));
        cx.run_until_parked();
    }
    let tabs: Vec<_> = ws.read_with(&*cx, |ws, _| {
        ws.changes_views
            .iter()
            .filter_map(|c| c.upgrade())
            .collect()
    });
    assert_eq!(tabs.len(), 1, "one tab per folder");
    let view = tabs[0].clone();
    wait_until(cx, "first load", |cx| {
        view.read_with(cx, |c, _| c.changes.is_some())
    });

    std::fs::write(f.project.join("second.txt"), "from the second thread\n").expect("write");
    second.update(cx, |_, cx| cx.emit(ThreadViewEvent::TurnDone));
    wait_until(cx, "reload on the second thread's TurnDone", |cx| {
        view.read_with(cx, |c, _| {
            c.changes.as_ref().is_some_and(|c| c.files.len() == 1)
        })
    });
}

#[test]
fn worktrees_made_at_once_with_one_title_all_get_their_own() {
    // A multi-model send makes one per model in the same moment, from the same prompt.
    let f = super::worktree_thread_test::fixture();
    let made: Vec<PathBuf> = std::thread::scope(|s| {
        let jobs: Vec<_> = (0..4)
            .map(|_| {
                s.spawn(|| worktree::create(&f.config, &f.project, "acme", "same", None, None))
            })
            .collect();
        jobs.into_iter()
            .map(|j| j.join().expect("thread").expect("create").path)
            .collect()
    });
    let unique: std::collections::HashSet<_> = made.iter().collect();
    assert_eq!(unique.len(), 4, "{made:?}");
}

#[test]
fn a_slow_checkout_in_one_repo_does_not_hold_up_another() {
    let (a, b) = (
        super::worktree_thread_test::fixture(),
        super::worktree_thread_test::fixture(),
    );
    let a_root = std::fs::canonicalize(&a.project).expect("canonical");
    let held = worktree::creating(&a_root);
    let _busy = held.lock().expect("lock"); // repo A is making a worktree
    let (tx, rx) = std::sync::mpsc::channel();
    let (config, project) = (b.config.clone(), b.project.clone());
    std::thread::spawn(move || {
        let made = worktree::create(&config, &project, "acme", "other repo", None, None);
        let _ = tx.send(made.map(|m| m.path));
    });
    let made = rx.recv_timeout(std::time::Duration::from_secs(30));
    assert!(
        matches!(made, Ok(Ok(_))),
        "repo B waited on repo A: {made:?}"
    );
}
