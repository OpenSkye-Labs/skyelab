//! Follow-ups to worktree threads: ignored files on discard, the diff base, a Changes tab shared
//! by threads, and the cleanup when a new thread's row can't be saved.

use super::changes_model_test::{git, repo};
use super::worktree::{self, Discard, discard};
use std::path::{Path, PathBuf};

fn add_worktree(root: &Path, name: &str) -> PathBuf {
    let wt = root.join(".skyelab/worktrees").join(name);
    let branch = format!("skyelab/{name}");
    let dest = wt.to_str().expect("utf-8");
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
    let created = worktree::create(&f.config, &f.project, "acme", "doomed").expect("create");
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
    let err = worktree::save(&f.store, &f.project, &new, Some(created))
        .expect_err("the row can't be saved");
    assert!(!path.exists(), "worktree left behind after: {err:#}");
}
