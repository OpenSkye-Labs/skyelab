//! Push, Pull and fetch against an https remote, the way they reach GitHub: TLS checked against a CA only the child
//! git trusts, credentials only from the bound (fake) gh. The remote is `support/https_remote.rs` on 127.0.0.1.
#![cfg(all(unix, debug_assertions))] // the fake gh is a shell script; the CA hook is debug-only

#[path = "support/https_remote.rs"]
mod https_remote;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use https_remote::HttpsRemote;
use skyelab_git::GitHub;

/// One server (and CA) per test binary, serving the bare repos under its folder.
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

fn fake_gh() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh")
}

struct Fixture {
    _tmp: tempfile::TempDir,
    /// The remote's bare repo, as the server serves it.
    bare: PathBuf,
    url: String,
    /// Ours, with `origin` at `url`.
    repo: PathBuf,
    gh_dir: PathBuf,
}

impl Fixture {
    /// A fresh bare repo `name.git` on the server and a local repo with one commit on `main`, not pushed yet.
    fn new(name: &str) -> Self {
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
        let base = std::fs::canonicalize(tmp.path()).expect("canon");
        let (repo, gh_dir) = (base.join("repo"), base.join("gh"));
        std::fs::create_dir_all(&repo).expect("mkdir");
        std::fs::create_dir_all(&gh_dir).expect("mkdir");
        git(&repo, &["init", "-q", "-b", "main"]);
        configure(&repo);
        std::fs::write(repo.join("a.txt"), "a\n").expect("w");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "init"]);
        let url = format!("{}/{name}.git", remote.base);
        git(&repo, &["remote", "add", "origin", &url]);
        Self {
            _tmp: tmp,
            bare,
            url,
            repo,
            gh_dir,
        }
    }

    fn gh(&self) -> (PathBuf, PathBuf) {
        (fake_gh(), self.gh_dir.clone())
    }

    /// Someone else moves the remote's `main` on by one commit (straight into the bare repo).
    fn advance_remote(&self) -> String {
        let other = self._tmp.path().join("other");
        git(
            self._tmp.path(),
            &["clone", "-q", self.bare.to_str().expect("utf8"), "other"],
        );
        configure(&other);
        std::fs::write(other.join("b.txt"), "b\n").expect("w");
        git(&other, &["add", "."]);
        git(&other, &["commit", "-q", "-m", "theirs"]);
        git(&other, &["push", "-q", "origin", "main"]);
        git(&other, &["rev-parse", "HEAD"])
    }
}

fn configure(repo: &Path) {
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(repo, &["config", k, v]);
    }
}

fn requests_to(url_path: &str) -> Vec<https_remote::Seen> {
    let (remote, _) = server();
    let seen = remote.seen.lock().expect("seen").clone();
    seen.into_iter()
        .filter(|s| s.target.starts_with(url_path))
        .collect()
}

#[test]
fn push_sets_upstream_with_gh_credentials() {
    let f = Fixture::new("push");
    let (gh, dir) = f.gh();
    let gh = GitHub {
        gh: &gh,
        config_dir: &dir,
    };
    assert_eq!(
        skyelab_git::push(&f.repo, &gh).expect("push"),
        "origin/main"
    );
    assert_eq!(
        git(&f.bare, &["rev-parse", "main"]),
        git(&f.repo, &["rev-parse", "HEAD"]),
        "the remote has our commit"
    );
    assert_eq!(git(&f.repo, &["config", "branch.main.remote"]), "origin");
    let seen = requests_to("/push.git/");
    assert!(
        seen.iter()
            .any(|s| s.authorized && s.method == "POST" && s.target.ends_with("/git-receive-pack")),
        "an authorized receive-pack: {seen:?}"
    );
    assert!(
        dir.join("fake-gh.credential").is_file(),
        "the credentials came from the bound gh"
    );
}

#[test]
fn fetch_updates_tracking_and_pull_fast_forwards() {
    let f = Fixture::new("pull");
    let (gh, dir) = f.gh();
    let gh = GitHub {
        gh: &gh,
        config_dir: &dir,
    };
    skyelab_git::push(&f.repo, &gh).expect("push");
    let theirs = f.advance_remote();

    assert!(skyelab_git::fetch(&f.repo, &gh).expect("fetch"), "fetched");
    assert_eq!(git(&f.repo, &["rev-parse", "origin/main"]), theirs);
    assert_eq!(
        skyelab_git::ahead_behind(&f.repo).expect("count"),
        Some((0, 1))
    );

    let msg = skyelab_git::pull_ff(&f.repo, &gh, &[]).expect("pull");
    assert_eq!(msg, "Pulled 1 commit(s) into main");
    assert_eq!(git(&f.repo, &["rev-parse", "HEAD"]), theirs);
    assert!(
        requests_to("/pull.git/")
            .iter()
            .any(|s| s.authorized && s.target.ends_with("/git-upload-pack")),
        "fetched over https"
    );
}

#[test]
fn fetch_refuses_repo_set_keys_before_any_request() {
    let f = Fixture::new("refuse");
    let (gh, dir) = f.gh();
    let gh = GitHub {
        gh: &gh,
        config_dir: &dir,
    };
    skyelab_git::push(&f.repo, &gh).expect("push");
    let (remote, _) = server();
    let ca = remote.ca_file.to_str().expect("utf8").to_owned();
    let scoped = format!("http.{}.sslcainfo", f.url);
    for (key, value) in [
        ("remote.origin.uploadpack", "git-upload-pack"),
        ("remote.origin.proxy", "http://127.0.0.1:9"),
        ("http.proxy", "http://127.0.0.1:9"),
        ("http.sslcainfo", ca.as_str()),
        (scoped.as_str(), ca.as_str()),
    ] {
        git(&f.repo, &["config", key, value]);
        let before = requests_to("/refuse.git/").len();
        let err = skyelab_git::fetch(&f.repo, &gh).expect_err(key);
        assert!(err.to_string().contains("won't fetch"), "{key}: {err}");
        let pull = skyelab_git::pull_ff(&f.repo, &gh, &[]).expect_err(key);
        assert!(pull.to_string().contains("won't fetch"), "{key}: {pull}");
        assert_eq!(
            requests_to("/refuse.git/").len(),
            before,
            "{key}: nothing reached the remote"
        );
        git(&f.repo, &["config", "--unset", key]);
    }
    assert!(skyelab_git::fetch(&f.repo, &gh).expect("clean config fetches"));
}

#[test]
fn the_ca_is_trusted_only_where_skyelab_says_so() {
    let f = Fixture::new("trust");
    // Plain git, without SkyeLab's env: the system's CAs don't know this one.
    let out = Command::new("git")
        .args(["ls-remote", &f.url])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr).to_lowercase();
    assert!(err.contains("certificate"), "{err}");
    // Trusting the CA but sending no credentials: the remote answers 401.
    let (remote, _) = server();
    let out = Command::new("git")
        .args(["-c", "credential.helper=", "ls-remote", &f.url])
        .env("GIT_SSL_CAINFO", &remote.ca_file)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    assert!(!out.status.success(), "no token, no access");
    assert!(
        requests_to("/trust.git/").iter().any(|s| !s.authorized),
        "the remote saw the anonymous request"
    );
}

/// Push refuses the same repo-set keys as fetch (plus `receivepack`), before git starts: the server sees no request
/// and the bound gh is never asked for its token.
#[test]
fn push_refuses_repo_set_keys_before_the_token_leaves() {
    let (remote, _) = server();
    let ca = remote.ca_file.to_str().expect("utf8").to_owned();
    let keys = [
        ("remote.origin.uploadpack", "git-upload-pack".to_owned()),
        ("remote.origin.receivepack", "git-receive-pack".to_owned()),
        ("remote.origin.proxy", "http://127.0.0.1:9".to_owned()),
        ("remote.origin.vcs", "evil".to_owned()),
        ("http.proxy", "http://127.0.0.1:9".to_owned()),
        ("http.sslcainfo", ca.clone()),
        ("SCOPED.sslcainfo", ca.clone()),
        ("http.sslcapath", "/tmp".to_owned()),
        ("http.sslcert", "/tmp/c.pem".to_owned()),
        ("http.sslkey", "/tmp/k.pem".to_owned()),
        ("http.curloptresolve", "127.0.0.1:443:127.0.0.1".to_owned()),
        ("http.cookiefile", "/tmp/cookies".to_owned()),
        ("http.pinnedpubkey", "sha256//x".to_owned()),
        ("core.alternaterefscommand", "true".to_owned()),
    ];
    for (key, value) in keys {
        let f = Fixture::new(&format!("push-refuse-{}", key.replace('.', "-")));
        let key = key.replace("SCOPED", &format!("http.{}", f.url));
        git(&f.repo, &["config", &key, &value]);
        let (gh, dir) = f.gh();
        let gh = GitHub {
            gh: &gh,
            config_dir: &dir,
        };
        let err = skyelab_git::push(&f.repo, &gh).expect_err(&key);
        assert!(err.to_string().contains("won't push"), "{key}: {err}");
        assert!(
            err.to_string().to_lowercase().contains(&key.to_lowercase()),
            "{key}: the message names the key: {err}"
        );
        let path = format!(
            "/{}.git/",
            f.bare.file_stem().expect("name").to_string_lossy()
        );
        assert!(
            requests_to(&path).is_empty(),
            "{key}: nothing reached the remote"
        );
        assert!(
            !dir.join("fake-gh.credential").exists(),
            "{key}: the gh login was never asked for its token"
        );
        git(&f.repo, &["config", "--unset", &key]);
        skyelab_git::push(&f.repo, &gh).expect("a clean config pushes");
    }
}
