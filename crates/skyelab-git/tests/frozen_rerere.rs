//! A frozen commit (hooks off) that ends a conflicted merge records git's rerere resolution in the repo's own
//! `rr-cache`, not in the private common dir the frozen config lives in.
//! Runs on Windows too, where the link is a dir symlink or, without Developer Mode, a junction.

use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs")
        .status
        .success()
}

#[test]
fn a_frozen_merge_commit_keeps_its_rerere_record() {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = std::fs::canonicalize(tmp.path()).expect("canon");
    assert!(git(&repo, &["init", "-q", "-b", "main"]));
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
        ("rerere.enabled", "true"),
        ("core.autocrlf", "false"), // Windows runners turn it on globally
    ] {
        assert!(git(&repo, &["config", k, v]));
    }
    let file = repo.join("f.txt");
    std::fs::write(&file, "base\n").expect("w");
    assert!(git(&repo, &["add", "."]));
    assert!(git(&repo, &["commit", "-q", "-m", "base"]));
    assert!(git(&repo, &["checkout", "-q", "-b", "side"]));
    std::fs::write(&file, "side\n").expect("w");
    assert!(git(&repo, &["commit", "-q", "-am", "side"]));
    assert!(git(&repo, &["checkout", "-q", "main"]));
    std::fs::write(&file, "main\n").expect("w");
    assert!(git(&repo, &["commit", "-q", "-am", "main"]));
    assert!(!git(&repo, &["merge", "-q", "side"]), "the merge conflicts");
    std::fs::write(&file, "both\n").expect("w");
    assert!(git(&repo, &["add", "f.txt"]));

    let reviewed = skyelab_git::snapshot_tree(&repo).expect("snapshot");
    skyelab_git::commit(&repo, "merge", &[], &[], false, Some(&reviewed)).expect("merge commit");

    let cache = repo.join(".git/rr-cache");
    let ids: Vec<_> = std::fs::read_dir(&cache)
        .expect("rr-cache")
        .map(|e| e.expect("entry").path())
        .collect();
    assert_eq!(ids.len(), 1, "{ids:?}");
    assert_eq!(
        std::fs::read_to_string(ids[0].join("postimage")).expect("RERERE_LOST: no postimage"),
        "both\n"
    );
}
