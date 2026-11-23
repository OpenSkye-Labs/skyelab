//! Per-turn checkpoints on temp repos: captured under refs/skyelab/ without touching the user's HEAD, index or
//! branches; rewound exactly; never through a symlink or over an ignored file unless confirmed; cleaned up.

use std::path::{Path, PathBuf};
use std::process::Command;

use skyelab_git::{
    capture_checkpoint, checkpoints, delete_checkpoints, diff_trees, plan_rewind, rewind,
    snapshot_tree,
};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = std::fs::canonicalize(tmp.path())
        .expect("canon")
        .join("repo");
    std::fs::create_dir(&repo).expect("mkdir");
    git(&repo, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", k, v]);
    }
    std::fs::write(repo.join("a.txt"), "one\n").expect("w");
    std::fs::write(repo.join("gone.txt"), "will be deleted\n").expect("w");
    std::fs::write(repo.join(".gitignore"), ".env\n").expect("w");
    std::fs::create_dir(repo.join("dir")).expect("mkdir");
    std::fs::write(repo.join("dir/f.txt"), "in dir\n").expect("w");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    (tmp, repo)
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).expect("read")
}

/// Everything of the user's that a checkpoint must leave alone.
fn user_state(repo: &Path) -> (String, String, String, String) {
    (
        git(repo, &["rev-parse", "HEAD"]),
        git(repo, &["ls-files", "--stage"]),
        git(
            repo,
            &["for-each-ref", "refs/heads", "refs/tags", "refs/stash"],
        ),
        git(repo, &["status", "--porcelain"]),
    )
}

#[test]
fn capture_lists_by_turn_and_leaves_the_user_alone() {
    let (_tmp, repo) = repo();
    std::fs::write(repo.join("a.txt"), "staged\n").expect("w");
    git(&repo, &["add", "a.txt"]);
    std::fs::write(repo.join("untracked.txt"), "u\n").expect("w");
    let before = user_state(&repo);
    let t0 = capture_checkpoint(&repo, "7-100", 0).expect("turn 0");
    std::fs::write(repo.join("a.txt"), "agent edit\n").expect("w");
    let t1 = capture_checkpoint(&repo, "7-100", 1).expect("turn 1");
    std::fs::write(repo.join("a.txt"), "staged\n").expect("w"); // back as the user had it
    assert_eq!(user_state(&repo), before);

    let list = checkpoints(&repo, "7-100").expect("list");
    assert_eq!(list.iter().map(|c| c.turn).collect::<Vec<_>>(), [0, 1]);
    assert_eq!(
        (list[0].tree.as_str(), list[1].tree.as_str()),
        (t0.as_str(), t1.as_str())
    );
    let refs = git(
        &repo,
        &["for-each-ref", "--format=%(refname)", "refs/skyelab/"],
    );
    assert_eq!(
        refs.lines().collect::<Vec<_>>(),
        [
            "refs/skyelab/checkpoints/7-100/0",
            "refs/skyelab/checkpoints/7-100/1"
        ]
    );
    // The untracked file is in the snapshot; the turn's diff is just a.txt.
    let diff = diff_trees(&repo, &t0, &t1).expect("diff");
    assert_eq!(diff.len(), 1, "{diff:?}");
    assert_eq!(diff[0].new_path.as_deref(), Some(Path::new("a.txt")));
    assert!(checkpoints(&repo, "../x").is_err());
    assert!(capture_checkpoint(&repo, "a/b", 0).is_err());
}

#[test]
fn rewind_restores_exactly_and_keeps_ignored_files() {
    let (_tmp, repo) = repo();
    std::fs::write(repo.join(".env"), "SECRET=1\n").expect("w");
    // A change the user staged before the thread: it is in the checkpoint, and must stay staged.
    std::fs::write(repo.join("a.txt"), "staged by the user\n").expect("w");
    git(&repo, &["add", "a.txt"]);
    // After user_state: its `git status` may refresh the index's stat cache.
    let (head, _, branches, _) = user_state(&repo);
    let index_file = std::fs::read(repo.join(".git/index")).expect("index");
    let t0 = capture_checkpoint(&repo, "k", 0).expect("turn 0");
    // The turn: an edit, a delete, a new file in a new folder, a deleted folder, and an ignored file changed.
    std::fs::write(repo.join("a.txt"), "changed\n").expect("w");
    std::fs::remove_file(repo.join("gone.txt")).expect("rm");
    std::fs::create_dir_all(repo.join("new/deep")).expect("mkdir");
    std::fs::write(repo.join("new/deep/n.txt"), "new\n").expect("w");
    std::fs::remove_dir_all(repo.join("dir")).expect("rm dir");
    std::fs::write(repo.join(".env"), "SECRET=2\n").expect("w");

    let plan = plan_rewind(&repo, &t0).expect("plan");
    let mut restore = plan.restore.clone();
    restore.sort();
    assert_eq!(
        restore,
        [
            PathBuf::from("a.txt"),
            "dir/f.txt".into(),
            "gone.txt".into()
        ]
    );
    assert_eq!(plan.remove, [PathBuf::from("new/deep/n.txt")]);
    assert!(plan.ignored.is_empty());
    rewind(&repo, &plan, false).expect("rewind");

    assert_eq!(snapshot_tree(&repo).expect("snap"), t0);
    assert_eq!(read(&repo.join("a.txt")), "staged by the user\n");
    assert_eq!(read(&repo.join("gone.txt")), "will be deleted\n");
    assert_eq!(read(&repo.join("dir/f.txt")), "in dir\n");
    assert!(!repo.join("new").exists(), "empty folders it left go too");
    assert_eq!(
        read(&repo.join(".env")),
        "SECRET=2\n",
        "ignored files are never rewound"
    );
    assert_eq!(
        std::fs::read(repo.join(".git/index")).expect("index"),
        index_file,
        "the index is untouched"
    );
    let (head2, _, branches2, _) = user_state(&repo);
    assert_eq!((head2, branches2), (head, branches));
    assert_eq!(
        git(&repo, &["diff", "--cached", "--name-only"]),
        "a.txt\n",
        "the staged change is still staged"
    );
}

#[test]
fn an_existing_ignored_file_is_overwritten_only_when_confirmed() {
    let (_tmp, repo) = repo();
    std::fs::write(repo.join("notes.txt"), "at the checkpoint\n").expect("w");
    let t0 = capture_checkpoint(&repo, "k", 0).expect("turn 0");
    // The agent changes notes.txt and hides it from git.
    std::fs::write(repo.join("notes.txt"), "user's later notes\n").expect("w");
    std::fs::write(repo.join(".git/info/exclude"), "notes.txt\n").expect("w");

    let plan = plan_rewind(&repo, &t0).expect("plan");
    assert_eq!(plan.ignored, [PathBuf::from("notes.txt")]);
    assert!(!plan.restore.contains(&PathBuf::from("notes.txt")));
    rewind(&repo, &plan, false).expect("rewind without it");
    assert_eq!(read(&repo.join("notes.txt")), "user's later notes\n");

    let plan = plan_rewind(&repo, &t0).expect("plan again");
    rewind(&repo, &plan, true).expect("rewind with it");
    assert_eq!(read(&repo.join("notes.txt")), "at the checkpoint\n");
}

#[cfg(unix)]
#[test]
fn a_folder_swapped_for_a_symlink_is_never_written_through() {
    let (tmp, repo) = repo();
    let outside = tmp.path().join("outside");
    std::fs::create_dir(&outside).expect("mkdir");
    std::fs::write(outside.join("keep.txt"), "outside\n").expect("w");
    let t0 = capture_checkpoint(&repo, "k", 0).expect("turn 0");

    // Tracked folder replaced by a link out of the repo.
    std::fs::remove_dir_all(repo.join("dir")).expect("rm");
    std::os::unix::fs::symlink(&outside, repo.join("dir")).expect("link");
    let plan = plan_rewind(&repo, &t0).expect("plan");
    rewind(&repo, &plan, true).expect("rewind");
    assert!(
        !std::fs::symlink_metadata(repo.join("dir"))
            .expect("dir")
            .file_type()
            .is_symlink()
    );
    assert_eq!(read(&repo.join("dir/f.txt")), "in dir\n");
    let names: Vec<_> = std::fs::read_dir(&outside)
        .expect("ls")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(names, ["keep.txt"], "nothing was written outside the repo");

    // A tracked file swapped for a link to a file outside: the link is replaced, its target never written.
    std::fs::remove_file(repo.join("a.txt")).expect("rm");
    std::os::unix::fs::symlink(outside.join("keep.txt"), repo.join("a.txt")).expect("link");
    let plan = plan_rewind(&repo, &t0).expect("plan");
    rewind(&repo, &plan, true).expect("rewind");
    assert_eq!(read(&repo.join("a.txt")), "one\n");
    assert!(
        !std::fs::symlink_metadata(repo.join("a.txt"))
            .expect("a")
            .file_type()
            .is_symlink()
    );
    assert_eq!(read(&outside.join("keep.txt")), "outside\n");

    // The same, but the link is ignored, so it stays: the rewind must refuse rather than write through it.
    std::fs::remove_dir_all(repo.join("dir")).expect("rm");
    std::os::unix::fs::symlink(&outside, repo.join("dir")).expect("link");
    std::fs::write(repo.join(".git/info/exclude"), "/dir\n").expect("w");
    std::fs::write(outside.join("f.txt"), "outside f\n").expect("w");
    let plan = plan_rewind(&repo, &t0).expect("plan");
    assert!(rewind(&repo, &plan, true).is_err());
    assert_eq!(read(&outside.join("f.txt")), "outside f\n");
    assert_eq!(read(&outside.join("keep.txt")), "outside\n");
}

#[test]
fn a_change_after_the_plan_refuses_the_rewind() {
    let (_tmp, repo) = repo();
    let t0 = capture_checkpoint(&repo, "k", 0).expect("turn 0");
    std::fs::write(repo.join("a.txt"), "agent\n").expect("w");
    let plan = plan_rewind(&repo, &t0).expect("plan");
    std::fs::write(repo.join("later.txt"), "typed after the confirm showed\n").expect("w");
    assert!(rewind(&repo, &plan, false).is_err());
    assert_eq!(read(&repo.join("a.txt")), "agent\n");
    assert!(repo.join("later.txt").exists());
}

#[test]
fn delete_checkpoints_removes_only_that_threads_refs() {
    let (_tmp, repo) = repo();
    for turn in 0..3 {
        capture_checkpoint(&repo, "1-5", turn).expect("capture");
    }
    let kept = capture_checkpoint(&repo, "2-6", 0).expect("other thread");
    // The refs keep the snapshots alive through a gc that prunes everything unreachable.
    std::fs::write(repo.join("scratch.txt"), "only in a checkpoint\n").expect("w");
    let only = capture_checkpoint(&repo, "1-5", 3).expect("capture");
    std::fs::remove_file(repo.join("scratch.txt")).expect("rm");
    git(&repo, &["gc", "-q", "--prune=now"]);
    git(&repo, &["cat-file", "-e", &format!("{only}^{{tree}}")]);
    git(&repo, &["cat-file", "-e", &format!("{kept}^{{tree}}")]);
    delete_checkpoints(&repo, "1-5").expect("delete");
    assert!(checkpoints(&repo, "1-5").expect("list").is_empty());
    let other = checkpoints(&repo, "2-6").expect("list");
    assert_eq!(other.len(), 1);
    assert_eq!(other[0].tree, kept);
    git(&repo, &["cat-file", "-e", &kept]);
    delete_checkpoints(&repo, "1-5").expect("deleting nothing is fine");
}
