//! Proof: Switch with Stash never runs a filter a submodule's own config defines.
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

/// A repo with submodule `sub` whose own config names a clean filter, and a dirty file in `sub` the filter would hash.
fn rig(base: &Path, ignore_none: bool) -> std::path::PathBuf {
    let (sub, repo) = (base.join("sub"), base.join("repo"));
    git(base, &["init", "-q", "-b", "main", "sub"]);
    configure(&sub);
    fs::write(sub.join(".gitattributes"), "*.txt filter=sev\n").expect("attrs");
    fs::write(sub.join("a.txt"), "1\n").expect("write");
    git(&sub, &["add", "."]);
    git(&sub, &["commit", "-q", "-m", "init"]);
    git(base, &["init", "-q", "-b", "main", "repo"]);
    configure(&repo);
    fs::write(repo.join("b.txt"), "1\n").expect("write");
    git(&repo, &["add", "b.txt"]);
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
    if ignore_none {
        git(
            &repo,
            &[
                "config",
                "-f",
                ".gitmodules",
                "submodule.sub.ignore",
                "none",
            ],
        );
        git(&repo, &["add", ".gitmodules"]);
    }
    git(&repo, &["commit", "-q", "-m", "add sub"]);
    git(&repo, &["branch", "other"]);
    fs::write(repo.join("c.txt"), "1\n").expect("write");
    git(&repo, &["add", "c.txt"]);
    git(&repo, &["commit", "-q", "-m", "c"]);
    let evil = base.join("evil");
    fs::write(
        &evil,
        format!(
            "#!/bin/sh\necho clean >> '{}'\ncat\n",
            base.join("ran").display()
        ),
    )
    .expect("evil");
    fs::set_permissions(&evil, fs::Permissions::from_mode(0o755)).expect("chmod");
    git(
        &repo.join("sub"),
        &["config", "filter.sev.clean", evil.to_str().expect("utf-8")],
    );
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(repo.join("sub/a.txt"), "2\n").expect("same size, newer mtime");
    repo
}

#[test]
fn stash_switch_never_runs_a_submodules_filter() {
    for ignore_none in [false, true] {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let base = fs::canonicalize(tmp.path()).expect("canonical");
        let repo = rig(&base, ignore_none);
        fs::write(repo.join("b.txt"), "2\n").expect("dirty");
        let _ = skyelab_git::switch_branch(&repo, "other", skyelab_git::OnDirty::Stash);
        assert_eq!(
            fs::read_to_string(base.join("ran")).unwrap_or_default(),
            "",
            "stash ({ignore_none})"
        );
    }
}

#[test]
fn switch_never_runs_a_submodules_filter() {
    for ignore_none in [false, true] {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let base = fs::canonicalize(tmp.path()).expect("canonical");
        let repo = rig(&base, ignore_none);
        let _ = skyelab_git::switch_branch(&repo, "other", skyelab_git::OnDirty::Refuse);
        let _ = skyelab_git::status(&repo);
        let _ = skyelab_git::snapshot_tree(&repo);
        assert_eq!(
            fs::read_to_string(base.join("ran")).unwrap_or_default(),
            "",
            "switch ({ignore_none})"
        );
    }
}
