//! Acceptance: a new project's clone takes https URLs only, gets credentials only from the bound (fake) gh, and with
//! no gh asks nobody (the user's own credential helper never runs). The remote is `support/https_remote.rs`.
#![cfg(all(unix, debug_assertions))] // the fake gh is a shell script; the CA hook is debug-only

#[path = "support/https_remote.rs"]
mod https_remote;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use https_remote::HttpsRemote;
use skyelab_git::GitHub;

fn server() -> &'static (HttpsRemote, PathBuf) {
    static SERVER: OnceLock<(HttpsRemote, PathBuf)> = OnceLock::new();
    SERVER.get_or_init(|| {
        let root = Box::leak(Box::new(tempfile::tempdir().expect("root")));
        let root = std::fs::canonicalize(root.path()).expect("canon");
        let remote = HttpsRemote::start(&root);
        skyelab_git::trust_test_ca(&remote.ca_file);
        (remote, root)
    })
}

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

/// A bare repo `name.git` on the server with one commit on `main`; its https URL.
fn remote_repo(name: &str) -> String {
    let (remote, root) = server();
    let bare = root.join(format!("{name}.git"));
    git(
        root,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            bare.to_str().expect("utf8"),
        ],
    );
    let tmp = tempfile::tempdir().expect("tmp");
    let work = tmp.path().join("w");
    std::fs::create_dir_all(&work).expect("mkdir");
    git(&work, &["init", "-q", "-b", "main"]);
    std::fs::write(work.join("a.txt"), "a\n").expect("write");
    git(&work, &["add", "."]);
    git(
        &work,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "init",
        ],
    );
    git(&work, &["push", "-q", bare.to_str().expect("utf8"), "main"]);
    format!("{}/{name}.git", remote.base)
}

fn fake_gh() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh")
}

#[test]
fn clone_refuses_anything_but_plain_https() {
    let tmp = tempfile::tempdir().expect("tmp");
    for url in [
        "git@github.com:acme/api.git",
        "http://github.com/acme/api",
        "https://user:pw@github.com/acme/api",
        "file:///tmp/x",
    ] {
        let err = skyelab_git::clone(url, &tmp.path().join("x"), None).expect_err(url);
        assert!(!err.to_string().is_empty());
        assert!(!tmp.path().join("x").exists());
    }
}

#[test]
fn clone_uses_the_bound_gh_and_nothing_else() {
    let url = remote_repo("private");
    let tmp = tempfile::tempdir().expect("tmp");
    let base = std::fs::canonicalize(tmp.path()).expect("canon");
    let gh_dir = base.join("gh");
    std::fs::create_dir_all(&gh_dir).expect("mkdir");
    let gh_bin = fake_gh();
    let gh = GitHub {
        gh: &gh_bin,
        config_dir: &gh_dir,
    };
    let dest = base.join("code/private");
    skyelab_git::clone(&url, &dest, Some(&gh)).expect("clones with the account's gh");
    assert_eq!(
        std::fs::read_to_string(dest.join("a.txt")).expect("read"),
        "a\n"
    );
    let seen = server().0.seen.lock().expect("log").clone();
    assert!(
        seen.iter()
            .any(|s| s.method == "POST" && s.target.contains("private.git") && s.authorized),
        "the fake gh's token reached the remote"
    );

    // No gh: the remote wants a login, nobody answers, and no folder is left behind.
    let again = base.join("code/again");
    let err = skyelab_git::clone(&url, &again, None).expect_err("private repo, no login");
    assert!(err.to_string().contains("git clone"), "{err}");
    assert!(!again.exists(), "a failed clone leaves nothing");
}

/// With no gh, the clone's config clears every helper the user has: a decoy in the global config is never asked.
#[test]
fn clone_without_gh_never_asks_the_users_credential_helper() {
    let tmp = tempfile::tempdir().expect("tmp");
    let marker = tmp.path().join("decoy-was-asked");
    let global = tmp.path().join("global.gitconfig");
    std::fs::write(
        &global,
        format!(
            "[credential]\n\thelper = \"!f() {{ touch '{m}'; }}; f\"\n",
            m = marker.display()
        ),
    )
    .expect("write");
    let args = skyelab_git::clone_config_args(None, "https://github.com/acme/api").expect("args");
    let mut child = Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(&args)
        .args(["credential", "fill"])
        .env("GIT_CONFIG_GLOBAL", &global)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("git credential");
    std::io::Write::write_all(
        child.stdin.as_mut().expect("stdin"),
        b"protocol=https\nhost=github.com\n\n",
    )
    .expect("write");
    let out = child.wait_with_output().expect("wait");
    assert!(!out.status.success(), "nobody answers");
    assert!(!marker.exists(), "the global helper was asked");
}
