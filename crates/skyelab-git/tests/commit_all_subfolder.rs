//! Commit all from a project that is a subfolder of a bigger repo commits that folder only.
use std::path::Path;
use std::process::Command;

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
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[test]
fn commit_all_in_a_subfolder_leaves_the_rest_of_the_repo() {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = std::fs::canonicalize(tmp.path()).expect("canon");
    git(&repo, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", k, v]);
    }
    for d in ["app", "other"] {
        std::fs::create_dir(repo.join(d)).expect("mkdir");
        std::fs::write(repo.join(d).join("a.txt"), "one\n").expect("w");
    }
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    std::fs::write(repo.join("app/a.txt"), "two\n").expect("w");
    std::fs::write(repo.join("app/new.txt"), "new\n").expect("w");
    std::fs::write(repo.join("other/a.txt"), "two\n").expect("w");

    let tree = skyelab_git::snapshot_tree(&repo).expect("snapshot");
    skyelab_git::commit(&repo.join("app"), "app only", &[], &[], Some(&tree)).expect("commit all");

    let committed = git(&repo, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(
        committed.lines().collect::<Vec<_>>(),
        ["app/a.txt", "app/new.txt"]
    );
    assert_eq!(
        git(&repo, &["status", "--porcelain"]),
        "M other/a.txt",
        "the rest of the repo is untouched"
    );
}
