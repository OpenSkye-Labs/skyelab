//! Proof: snapshot_tree skips a non-UTF-8 gitlink too, so its own filter never runs. Linux only: APFS refuses the name.
#![cfg(target_os = "linux")]
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::Command;

fn git<S: AsRef<OsStr>>(dir: &Path, args: &[S]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git: {}",
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
fn non_utf8_gitlink_filter_never_runs() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let base = fs::canonicalize(tmp.path()).expect("canonical");
    let repo = base.join("repo");
    git(&base, &["init", "-q", "-b", "main", "repo"]);
    configure(&repo);
    let name = OsStr::from_bytes(b"sub\xff");
    let sub = repo.join(name);
    fs::create_dir(&sub).expect("sub dir");
    git(&sub, &["init", "-q", "-b", "main"]);
    configure(&sub);
    fs::write(sub.join(".gitattributes"), "*.txt filter=sev\n").expect("attrs");
    fs::write(sub.join("a.txt"), "1\n").expect("write");
    git(&sub, &["add", "."]);
    git(&sub, &["commit", "-q", "-m", "init"]);
    git(&repo, &[OsStr::new("add"), OsStr::new("--"), name]);
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
        &sub,
        &["config", "filter.sev.clean", evil.to_str().expect("utf-8")],
    );
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(sub.join("a.txt"), "2\n").expect("same size, newer mtime");
    let _ = skyelab_git::snapshot_tree(&repo);
    assert_eq!(
        fs::read_to_string(&marker).unwrap_or_default(),
        "",
        "a non-UTF-8 submodule's filter ran"
    );
}
