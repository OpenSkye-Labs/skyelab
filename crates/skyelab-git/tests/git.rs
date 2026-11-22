//! Every test builds its own repo in a temp dir. None touches the repo this crate lives in.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use skyelab_git::{ChangeKind, Error, Line};
use tempfile::TempDir;

/// Runs git for test setup with a fixed identity, so the user's global config can't break commits.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A repo on branch `main` with one commit holding `a.txt`.
fn repo() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().join("repo");
    fs::create_dir(&root).expect("mkdir");
    git(&root, &["init", "-q", "-b", "main"]);
    fs::write(root.join("a.txt"), "one\ntwo\nthree\n").expect("write");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "init"]);
    let root = fs::canonicalize(root).expect("canonicalize");
    (tmp, root)
}

#[test]
fn repo_root_finds_toplevel_from_subdir() {
    let (_tmp, root) = repo();
    let sub = root.join("deep/er");
    fs::create_dir_all(&sub).expect("mkdir");
    assert_eq!(skyelab_git::repo_root(&sub).expect("root"), root);
}

#[test]
fn repo_root_outside_a_repo_is_not_a_repo() {
    let tmp = TempDir::new().expect("tempdir");
    let err = skyelab_git::repo_root(tmp.path()).expect_err("not a repo");
    assert!(matches!(err, Error::NotARepo(_)), "{err:?}");
}

#[test]
fn worktree_round_trip() {
    let (tmp, root) = repo();
    let dest = tmp.path().join("wt one");
    let wt = skyelab_git::create_worktree(&root, "thread/1", "main", Some(&dest), false)
        .expect("create");
    assert!(wt.join("a.txt").exists());

    let list = skyelab_git::list_worktrees(&root).expect("list");
    assert_eq!(list.len(), 2, "{list:?}");
    assert_eq!(list[0].path, root);
    assert_eq!(list[0].branch.as_deref(), Some("main"));
    let made = list
        .iter()
        .find(|w| w.branch.as_deref() == Some("thread/1"))
        .expect("new worktree listed");
    assert_eq!(
        fs::canonicalize(&made.path).expect("canon"),
        fs::canonicalize(&wt).expect("canon")
    );
    assert_eq!(made.head.as_ref().map(String::len), Some(40));

    skyelab_git::remove_worktree(&root, &wt, false).expect("remove clean worktree");
    assert!(!wt.exists());
    assert_eq!(skyelab_git::list_worktrees(&root).expect("list").len(), 1);
}

#[test]
fn branch_exists_refused_unless_reused() {
    let (tmp, root) = repo();
    git(&root, &["branch", "taken"]);
    let dest = tmp.path().join("wt");
    let err = skyelab_git::create_worktree(&root, "taken", "main", Some(&dest), false)
        .expect_err("exists");
    assert!(
        matches!(&err, Error::BranchExists(b) if b == "taken"),
        "{err:?}"
    );
    assert!(!dest.exists());

    let wt =
        skyelab_git::create_worktree(&root, "taken", "main", Some(&dest), true).expect("reuse");
    assert!(wt.join("a.txt").exists());
}

#[test]
fn dirty_worktree_refused() {
    let (tmp, root) = repo();
    let wt = skyelab_git::create_worktree(&root, "t", "main", Some(&tmp.path().join("wt")), false)
        .expect("create");
    fs::write(wt.join("new.txt"), "x").expect("write");

    let err = skyelab_git::remove_worktree(&root, &wt, false).expect_err("dirty");
    assert!(matches!(err, Error::Dirty(_)), "{err:?}");
    assert!(wt.join("new.txt").exists());

    skyelab_git::remove_worktree(&root, &wt, true).expect("forced");
    assert!(!wt.exists());
}

#[test]
fn status_tricky_paths() {
    let (_tmp, root) = repo();
    fs::write(root.join("with space.txt"), "s\n").expect("write");
    fs::write(root.join("ünï cødé.txt"), "u\n").expect("write");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "more"]);

    git(&root, &["mv", "with space.txt", "renamed space.txt"]);
    fs::write(root.join("ünï cødé.txt"), "changed\n").expect("write");
    fs::write(root.join("new file ✓.txt"), "n\n").expect("write");
    fs::create_dir(root.join("sub")).expect("mkdir");

    // Run from a subdir: paths must still be repo-relative.
    let st = skyelab_git::status(&root.join("sub")).expect("status");
    assert_eq!(st.staged.len(), 1, "{st:?}");
    assert_eq!(st.staged[0].kind, ChangeKind::Renamed);
    assert_eq!(st.staged[0].path, Path::new("renamed space.txt"));
    assert_eq!(
        st.staged[0].orig_path.as_deref(),
        Some(Path::new("with space.txt"))
    );
    assert_eq!(st.unstaged.len(), 1, "{st:?}");
    assert_eq!(st.unstaged[0].kind, ChangeKind::Modified);
    assert_eq!(st.unstaged[0].path, Path::new("ünï cødé.txt"));
    assert_eq!(st.untracked, vec![PathBuf::from("new file ✓.txt")]);
    assert!(!st.is_clean());
}

#[test]
fn diff_kinds() {
    let (_tmp, root) = repo();
    fs::write(root.join("gone.txt"), "bye\n").expect("write");
    fs::write(
        root.join("old name ü.txt"),
        "alpha\nbeta\ngamma\ndelta\nepsilon\n",
    )
    .expect("write");
    fs::write(root.join("blob.bin"), [0u8, 1, 2, 3]).expect("write");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "base"]);

    fs::write(root.join("a.txt"), "one\nTWO\nthree\n").expect("write");
    fs::remove_file(root.join("gone.txt")).expect("rm");
    git(&root, &["mv", "old name ü.txt", "new name ü.txt"]);
    fs::write(
        root.join("new name ü.txt"),
        "alpha\nbeta\ngamma\ndelta\nepsilon!\n",
    )
    .expect("write");
    fs::write(root.join("blob.bin"), [0u8, 9, 9, 9]).expect("write");
    fs::write(root.join("added file.txt"), "hi\n").expect("write");
    git(&root, &["add", "-N", "added file.txt"]);

    let diffs = skyelab_git::diff(&root, "HEAD").expect("diff");
    let find = |p: &str| {
        diffs
            .iter()
            .find(|d| {
                d.new_path.as_deref() == Some(Path::new(p))
                    || d.old_path.as_deref() == Some(Path::new(p))
            })
            .unwrap_or_else(|| panic!("{p} missing from {diffs:#?}"))
    };

    let m = find("a.txt");
    assert_eq!(m.kind, ChangeKind::Modified);
    assert_eq!(m.hunks.len(), 1);
    let h = &m.hunks[0];
    assert_eq!(
        (h.old_start, h.old_lines, h.new_start, h.new_lines),
        (1, 3, 1, 3)
    );
    assert_eq!(
        h.lines,
        vec![
            Line::Context("one".into()),
            Line::Removed("two".into()),
            Line::Added("TWO".into()),
            Line::Context("three".into()),
        ]
    );

    let d = find("gone.txt");
    assert_eq!(d.kind, ChangeKind::Deleted);
    assert_eq!(d.new_path, None);
    assert_eq!(d.hunks[0].lines, vec![Line::Removed("bye".into())]);

    let r = find("new name ü.txt");
    assert_eq!(r.kind, ChangeKind::Renamed);
    assert_eq!(r.old_path.as_deref(), Some(Path::new("old name ü.txt")));
    assert!(r.hunks[0].lines.contains(&Line::Added("epsilon!".into())));

    let b = find("blob.bin");
    assert!(b.binary);
    assert!(b.hunks.is_empty());
    assert_eq!(b.kind, ChangeKind::Modified);

    let a = find("added file.txt");
    assert_eq!(a.kind, ChangeKind::Added);
    assert_eq!(a.old_path, None);
    assert_eq!(a.hunks[0].lines, vec![Line::Added("hi".into())]);

    assert_eq!(diffs.len(), 5, "{diffs:#?}");
}
