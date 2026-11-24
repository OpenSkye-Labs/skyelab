//! What gets committed is what the user reviewed. A file whose content git sees change after the review
//! check (an agent still writing it; modelled here by the repo's own clean filter answering differently) must
//! not have its later content committed, for ticked files and for commit-all. SkyeLab never runs a filter from the
//! repo's own config, so the review and the commit both see the file as written.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
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
    String::from_utf8_lossy(&out.stdout).into_owned()
}

struct Rig {
    _tmp: tempfile::TempDir,
    repo: PathBuf,
    counter: PathBuf,
    limit: PathBuf,
}

/// A repo where `t.txt` goes through a clean filter set in the repo's own config. The filter passes content through
/// for its first `limit` runs (the number in the `limit` file), then answers `LATER` instead.
fn rig() -> Rig {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = std::fs::canonicalize(tmp.path()).expect("canon");
    let repo = root.join("repo");
    std::fs::create_dir(&repo).expect("mkdir");
    let (counter, limit) = (root.join("runs"), root.join("limit"));
    let filter = root.join("flip.sh");
    std::fs::write(
        &filter,
        format!(
            "#!/bin/sh\nn=$(cat '{c}' 2>/dev/null || echo 0); n=$((n+1)); echo $n > '{c}'\n\
             if [ \"$n\" -le \"$(cat '{l}' 2>/dev/null || echo 1000000)\" ]; then cat; else cat >/dev/null; echo LATER; fi\n",
            c = counter.display(),
            l = limit.display()
        ),
    )
    .expect("filter");
    std::fs::set_permissions(&filter, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    git(&repo, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
        ("filter.flip.clean", filter.to_str().expect("utf-8")),
    ] {
        git(&repo, &["config", k, v]);
    }
    std::fs::write(repo.join(".gitattributes"), "t.txt filter=flip\n").expect("w");
    std::fs::write(repo.join("t.txt"), "first\n").expect("w");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    Rig {
        _tmp: tmp,
        repo,
        counter,
        limit,
    }
}

/// Arms the filter so any run of it answers `LATER`, then edits t.txt and reviews it.
fn reviewed(r: &Rig) -> String {
    std::fs::write(&r.limit, "0").expect("limit");
    std::fs::write(r.repo.join("t.txt"), "reviewed\n").expect("w");
    let _ = std::fs::remove_file(&r.counter);
    skyelab_git::snapshot_tree(&r.repo).expect("snapshot")
}

#[test]
fn a_ticked_file_commits_its_reviewed_content() {
    let r = rig();
    let tree = reviewed(&r);
    skyelab_git::commit(
        &r.repo,
        "tick",
        &[PathBuf::from("t.txt")],
        &[],
        true,
        Some(&tree),
    )
    .expect("committed");
    assert_eq!(git(&r.repo, &["show", "HEAD:t.txt"]), "reviewed\n");
}

#[test]
fn commit_all_commits_its_reviewed_content() {
    let r = rig();
    let tree = reviewed(&r);
    skyelab_git::commit(&r.repo, "all", &[], &[], true, Some(&tree)).expect("committed");
    assert_eq!(git(&r.repo, &["show", "HEAD:t.txt"]), "reviewed\n");
    assert_eq!(git(&r.repo, &["rev-parse", "HEAD^{tree}"]).trim(), tree);
}

#[test]
fn a_ticked_commit_leaves_other_staged_files_staged_and_out_of_it() {
    let r = rig();
    std::fs::write(r.repo.join("other.txt"), "staged by the user\n").expect("w");
    git(&r.repo, &["add", "other.txt"]);
    let tree = reviewed(&r);
    skyelab_git::commit(
        &r.repo,
        "tick",
        &[PathBuf::from("t.txt")],
        &[],
        true,
        Some(&tree),
    )
    .expect("committed");
    assert_eq!(
        git(
            &r.repo,
            &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]
        ),
        "t.txt\n"
    );
    assert_eq!(
        git(&r.repo, &["diff", "--cached", "--name-only"]),
        "other.txt\n"
    );
}

#[test]
fn a_ticked_commit_is_refused_during_a_merge() {
    let r = rig();
    git(&r.repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(r.repo.join("a.txt"), "side\n").expect("w");
    std::fs::write(r.repo.join("b.txt"), "side\n").expect("w");
    git(&r.repo, &["add", "a.txt", "b.txt"]);
    git(&r.repo, &["commit", "-q", "-m", "side"]);
    git(&r.repo, &["checkout", "-q", "main"]);
    std::fs::write(r.repo.join("main.txt"), "main\n").expect("w");
    git(&r.repo, &["add", "main.txt"]);
    git(&r.repo, &["commit", "-q", "-m", "main"]);
    git(&r.repo, &["merge", "-q", "--no-commit", "--no-ff", "side"]);
    let tree = skyelab_git::snapshot_tree(&r.repo).expect("snapshot");
    let err = skyelab_git::commit(
        &r.repo,
        "part",
        &[PathBuf::from("a.txt")],
        &[],
        true,
        Some(&tree),
    )
    .expect_err("a partial commit during a merge");
    assert!(format!("{err}").contains("merge"), "{err}");
    assert_eq!(
        git(&r.repo, &["rev-list", "--merges", "--count", "HEAD"]).trim(),
        "0"
    );
}

#[test]
fn a_ticked_commit_is_refused_during_a_cherry_pick_revert_or_rebase() {
    for op in ["cherry-pick", "revert", "rebase"] {
        let r = rig();
        let base = git(&r.repo, &["rev-parse", "HEAD"]).trim().to_owned();
        git(&r.repo, &["checkout", "-q", "-b", "side"]);
        std::fs::write(r.repo.join("a.txt"), "side\n").expect("w");
        git(&r.repo, &["add", "a.txt"]);
        git(&r.repo, &["commit", "-q", "-m", "side"]);
        git(&r.repo, &["checkout", "-q", "main"]);
        std::fs::write(r.repo.join("a.txt"), "main\n").expect("w");
        git(&r.repo, &["add", "a.txt"]);
        git(&r.repo, &["commit", "-q", "-m", "main"]);
        // Each stops with a conflict on a.txt, so the operation stays in progress.
        let args: Vec<&str> = match op {
            "cherry-pick" => vec!["cherry-pick", "side"],
            "revert" => vec!["revert", "--no-edit", "HEAD"],
            _ => vec!["rebase", "side"],
        };
        let _ = Command::new("git")
            .arg("-C")
            .arg(&r.repo)
            .args(&args)
            .output();
        if op == "revert" {
            // Reverting "main" over its own parent is clean, so make it stop: revert the first commit instead.
            let _ = Command::new("git")
                .arg("-C")
                .arg(&r.repo)
                .args(["revert", "--abort"])
                .output();
            let _ = Command::new("git")
                .arg("-C")
                .arg(&r.repo)
                .args(["revert", "--no-commit", &base])
                .output();
        }
        std::fs::write(r.repo.join("a.txt"), "resolved\n").expect("w");
        std::fs::write(r.repo.join("b.txt"), "b\n").expect("w");
        let tree = skyelab_git::snapshot_tree(&r.repo).expect("snapshot");
        let err = skyelab_git::commit(
            &r.repo,
            "part",
            &[PathBuf::from("b.txt")],
            &[],
            true,
            Some(&tree),
        )
        .expect_err(op);
        assert!(format!("{err}").contains(op), "{op}: {err}");
    }
}
