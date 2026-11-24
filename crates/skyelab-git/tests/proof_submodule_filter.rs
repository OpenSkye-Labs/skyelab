//! Proof: background status never runs a filter a submodule's own config defines.
#![cfg(unix)]
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::Command;

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

fn configure(dir: &Path) {
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(dir, &["config", k, v]);
    }
}

#[test]
fn background_status_and_diff_never_run_a_submodules_own_filter() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let base = fs::canonicalize(tmp.path()).expect("canonical");
    let (sub, repo) = (base.join("sub"), base.join("repo"));
    git(&base, &["init", "-q", "-b", "main", "sub"]);
    configure(&sub);
    fs::write(sub.join(".gitattributes"), "*.txt filter=sev\n").expect("attrs");
    fs::write(sub.join("a.txt"), "1\n").expect("write");
    git(&sub, &["add", "."]);
    git(&sub, &["commit", "-q", "-m", "init"]);
    git(&base, &["init", "-q", "-b", "main", "repo"]);
    configure(&repo);
    git(
        &repo,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            sub.to_str().expect("utf-8"),
            "sub",
        ],
    );
    git(&repo, &["commit", "-q", "-m", "add sub"]);
    let marker = base.join("ran");
    let evil = base.join("evil");
    fs::write(
        &evil,
        format!("#!/bin/sh\necho clean >> '{}'\ncat\n", marker.display()),
    )
    .expect("evil");
    fs::set_permissions(&evil, fs::Permissions::from_mode(0o755)).expect("chmod");
    git(
        &repo.join("sub"),
        &["config", "filter.sev.clean", evil.to_str().expect("utf-8")],
    );
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(repo.join("sub/a.txt"), "2\n").expect("same size, newer mtime");
    let _ = skyelab_git::status(&repo);
    let _ = skyelab_git::diff(&repo, "HEAD");
    let _ = skyelab_git::diff_stat(&repo, None);
    let _ = skyelab_git::snapshot_tree(&repo);
    assert_eq!(
        fs::read_to_string(&marker).unwrap_or_default(),
        "",
        "a submodule's repo-config filter ran"
    );
}
