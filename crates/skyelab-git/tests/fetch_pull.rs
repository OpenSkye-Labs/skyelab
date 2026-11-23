//! Acceptance: fetch the branch's upstream under the account's own gh credentials, count ahead/behind, and pull
//! fast-forward only. Remotes are local bare repos and `gh` is a fake; nothing reaches the network.
#![cfg(unix)] // the fake gh and the planted upload-pack are shell scripts

use skyelab_git::GitHub;
use std::fs;
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
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn fake_gh() -> &'static Path {
    Box::leak(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/fake-gh.sh")
            .into_boxed_path(),
    )
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

struct Fixture {
    _tmp: tempfile::TempDir,
    base: PathBuf,
    /// Ours: `main` tracking `origin/main`.
    repo: PathBuf,
    /// Someone else's clone, which pushes new commits to the remote.
    other: PathBuf,
    gh_dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let base = fs::canonicalize(tmp.path()).expect("canonical");
        let (remote, repo, other, gh_dir) = (
            base.join("remote.git"),
            base.join("repo"),
            base.join("other"),
            base.join("gh"),
        );
        fs::create_dir_all(&gh_dir).expect("mkdir");
        git(&base, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
        git(
            &base,
            &["clone", "-q", remote.to_str().expect("utf-8"), "other"],
        );
        configure(&other);
        git(&other, &["checkout", "-q", "-b", "main"]);
        fs::write(other.join("a.txt"), "one\n").expect("write");
        git(&other, &["add", "."]);
        git(&other, &["commit", "-q", "-m", "init"]);
        git(&other, &["push", "-q", "origin", "main"]);
        git(
            &base,
            &["clone", "-q", remote.to_str().expect("utf-8"), "repo"],
        );
        configure(&repo);
        Self {
            _tmp: tmp,
            base,
            repo,
            other,
            gh_dir,
        }
    }

    fn gh(&self) -> GitHub<'_> {
        GitHub {
            gh: fake_gh(),
            config_dir: &self.gh_dir,
        }
    }

    /// The other clone pushes one more commit.
    fn upstream_moves(&self, n: u32) {
        fs::write(self.other.join(format!("u{n}.txt")), "x\n").expect("write");
        git(&self.other, &["add", "."]);
        git(&self.other, &["commit", "-q", "-m", &format!("up {n}")]);
        git(&self.other, &["push", "-q", "origin", "main"]);
    }

    fn local_commit(&self) {
        fs::write(self.repo.join("mine.txt"), "x\n").expect("write");
        git(&self.repo, &["add", "."]);
        git(&self.repo, &["commit", "-q", "-m", "mine"]);
    }
}

#[test]
fn fetch_counts_behind_and_pull_fast_forwards() {
    let f = Fixture::new();
    assert_eq!(
        skyelab_git::ahead_behind(&f.repo).expect("counts"),
        Some((0, 0))
    );
    f.upstream_moves(1);
    f.upstream_moves(2);
    assert_eq!(
        skyelab_git::ahead_behind(&f.repo).expect("counts"),
        Some((0, 0)),
        "nothing fetched yet"
    );
    assert!(skyelab_git::fetch(&f.repo, &f.gh()).expect("fetches"));
    assert_eq!(
        skyelab_git::ahead_behind(&f.repo).expect("counts"),
        Some((0, 2))
    );
    let note = skyelab_git::pull_ff(&f.repo, &f.gh(), &[]).expect("pulls");
    assert!(note.contains('2'), "{note}");
    assert_eq!(
        git(&f.repo, &["rev-parse", "HEAD"]),
        git(&f.other, &["rev-parse", "HEAD"])
    );
    assert_eq!(
        skyelab_git::ahead_behind(&f.repo).expect("counts"),
        Some((0, 0))
    );
    let again = skyelab_git::pull_ff(&f.repo, &f.gh(), &[]).expect("nothing to pull");
    assert!(again.to_lowercase().contains("up to date"), "{again}");
}

#[test]
fn ahead_counts_local_commits_and_pull_refuses_a_non_fast_forward() {
    let f = Fixture::new();
    f.local_commit();
    f.upstream_moves(1);
    skyelab_git::fetch(&f.repo, &f.gh()).expect("fetches");
    assert_eq!(
        skyelab_git::ahead_behind(&f.repo).expect("counts"),
        Some((1, 1))
    );
    let head = git(&f.repo, &["rev-parse", "HEAD"]);
    let err = skyelab_git::pull_ff(&f.repo, &f.gh(), &[]).expect_err("diverged");
    assert!(err.to_string().contains("fast-forward"), "{err}");
    assert_eq!(git(&f.repo, &["rev-parse", "HEAD"]), head);
}

#[test]
fn pull_refuses_a_dirty_tree() {
    let f = Fixture::new();
    f.upstream_moves(1);
    fs::write(f.repo.join("a.txt"), "edited\n").expect("write");
    let head = git(&f.repo, &["rev-parse", "HEAD"]);
    let err = skyelab_git::pull_ff(&f.repo, &f.gh(), &[]).expect_err("dirty");
    assert!(err.to_string().contains("uncommitted"), "{err}");
    assert_eq!(git(&f.repo, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        fs::read_to_string(f.repo.join("a.txt")).expect("read"),
        "edited\n"
    );
}

#[test]
fn no_upstream_means_no_counts_no_fetch_and_no_pull() {
    let f = Fixture::new();
    git(&f.repo, &["checkout", "-q", "-b", "feature"]);
    assert_eq!(skyelab_git::ahead_behind(&f.repo).expect("counts"), None);
    assert!(!skyelab_git::fetch(&f.repo, &f.gh()).expect("no-op"));
    let err = skyelab_git::pull_ff(&f.repo, &f.gh(), &[]).expect_err("no upstream");
    assert!(err.to_string().contains("upstream"), "{err}");
}

#[test]
fn fetch_refuses_ssh_before_ssh_runs() {
    let f = Fixture::new();
    let marker = f.base.join("ssh-ran");
    let ssh = f.base.join("fake-ssh.sh");
    fs::write(
        &ssh,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )
    .expect("write");
    fs::set_permissions(&ssh, fs::Permissions::from_mode(0o755)).expect("chmod");
    git(
        &f.repo,
        &["config", "core.sshCommand", ssh.to_str().expect("utf-8")],
    );
    git(
        &f.repo,
        &[
            "remote",
            "set-url",
            "origin",
            "git@github.com:acme/demo.git",
        ],
    );
    let err = skyelab_git::fetch(&f.repo, &f.gh()).expect_err("ssh");
    assert!(err.to_string().contains("https"), "{err}");
    let err = skyelab_git::pull_ff(&f.repo, &f.gh(), &[]).expect_err("ssh");
    assert!(err.to_string().contains("https"), "{err}");
    assert!(!marker.exists(), "ssh never ran");
    git(
        &f.repo,
        &[
            "remote",
            "set-url",
            "origin",
            "https://tok@github.com/acme/demo.git",
        ],
    );
    assert!(
        skyelab_git::fetch(&f.repo, &f.gh()).is_err(),
        "token in URL"
    );
}

#[test]
fn isolation_fetch_refuses_a_repo_configured_upload_pack() {
    let f = Fixture::new();
    // An agent can write .git/config: a local remote runs uploadpack through sh, with the account's gh login in env.
    let marker = f.base.join("payload-ran");
    let pack = f.base.join("upload-pack.sh");
    fs::write(
        &pack,
        format!(
            "#!/bin/sh\ntouch '{}'\nexec git-upload-pack \"$@\"\n",
            marker.display()
        ),
    )
    .expect("write");
    fs::set_permissions(&pack, fs::Permissions::from_mode(0o755)).expect("chmod");
    git(
        &f.repo,
        &[
            "config",
            "remote.origin.uploadpack",
            pack.to_str().expect("utf-8"),
        ],
    );
    f.upstream_moves(1);
    let err = skyelab_git::fetch(&f.repo, &f.gh()).expect_err("refused");
    assert!(
        err.to_string().contains("remote.origin.uploadpack"),
        "{err}"
    );
    assert!(
        skyelab_git::pull_ff(&f.repo, &f.gh(), &[]).is_err(),
        "pull fetches first"
    );
    assert!(!marker.exists(), "the repo's upload-pack never ran");
    // A proxy with its own CA would read the token gh hands over; worktree config counts as the repo's own too.
    git(&f.repo, &["config", "--unset", "remote.origin.uploadpack"]);
    git(
        &f.repo,
        &[
            "config",
            "http.https://github.com/.proxy",
            "http://127.0.0.1:9",
        ],
    );
    let err = skyelab_git::fetch(&f.repo, &f.gh()).expect_err("refused");
    assert!(err.to_string().contains("proxy"), "{err}");
    git(
        &f.repo,
        &["config", "--unset", "http.https://github.com/.proxy"],
    );
    assert!(skyelab_git::fetch(&f.repo, &f.gh()).expect("fetches"));
}

#[test]
fn pull_keeps_an_ignored_local_file_the_upstream_starts_tracking() {
    let f = Fixture::new();
    fs::write(f.repo.join(".git/info/exclude"), "u1.txt\n").expect("exclude");
    fs::write(f.repo.join("u1.txt"), "my only copy\n").expect("write");
    f.upstream_moves(1);
    assert!(
        skyelab_git::pull_ff(&f.repo, &f.gh(), &[]).is_err(),
        "pull refuses to replace an ignored file"
    );
    assert_eq!(
        fs::read_to_string(f.repo.join("u1.txt")).expect("read"),
        "my only copy\n"
    );
}
