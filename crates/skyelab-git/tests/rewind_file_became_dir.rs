//! A tracked file the agent replaced with a folder comes back on rewind, and is not an "ignored" file.
use std::path::Path;
use std::process::Command;

use skyelab_git::{capture_checkpoint, checkpoints, plan_rewind, rewind};

fn git(dir: &Path, args: &[&str]) {
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
}

#[test]
fn a_file_replaced_by_a_folder_is_restored_not_skipped_as_ignored() {
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
    std::fs::write(repo.join("config"), "x=1\n").expect("w");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);

    capture_checkpoint(&repo, "9-100", 0).expect("turn 0");
    std::fs::remove_file(repo.join("config")).expect("rm");
    std::fs::create_dir(repo.join("config")).expect("mkdir");
    std::fs::write(repo.join("config/default.toml"), "x = 1\n").expect("w");
    capture_checkpoint(&repo, "9-100", 1).expect("turn 1");

    let to = checkpoints(&repo, "9-100").expect("list")[0].tree.clone();
    let plan = plan_rewind(&repo, &to).expect("plan");
    assert!(
        !plan.ignored.iter().any(|p| p == Path::new("config")),
        "tracked file `config` was listed as ignored: {plan:?}"
    );
    rewind(&repo, &plan, false).expect("rewind");
    assert_eq!(
        std::fs::read_to_string(repo.join("config")).ok(),
        Some("x=1\n".to_owned()),
        "config was not restored by the rewind"
    );
}

/// A case-only rename (one file on macOS's default file system) is undone, not skipped as an ignored file.
#[test]
fn a_case_only_rename_is_rewound() {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = std::fs::canonicalize(tmp.path())
        .expect("canon")
        .join("repo");
    std::fs::create_dir(&repo).expect("mkdir");
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("Readme.md"), "hi\n").expect("w");
    let t0 = capture_checkpoint(&repo, "k", 0).expect("turn 0");
    std::fs::rename(repo.join("Readme.md"), repo.join("README.md")).expect("rename");
    let plan = plan_rewind(&repo, &t0).expect("plan");
    assert!(plan.ignored.is_empty(), "{plan:?}");
    rewind(&repo, &plan, false).expect("rewind");
    let names: Vec<_> = std::fs::read_dir(&repo)
        .expect("ls")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .filter(|n| n != ".git")
        .collect();
    assert_eq!(names, ["Readme.md"]);
    assert_eq!(
        std::fs::read_to_string(repo.join("Readme.md")).expect("r"),
        "hi\n"
    );
}
