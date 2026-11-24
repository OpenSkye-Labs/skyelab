//! Acceptance: with hooks off (every background call, and Commit/Push/Pull in a repo another account shares), no
//! program the repo's own config names ever runs: filters, signing programs, `core.sshCommand`,
//! credential helpers, askpass, fsmonitor, hooks set in config. Each planted program writes a marker, which must stay empty. `gh pr create`
//! runs git in the repo too, so its git gets the same overrides. Remotes are local bare repos; `gh` is a fake.
#![cfg(unix)] // the planted programs and the fake gh are shell scripts

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use skyelab_git::GitHub;

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

fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("write script");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod");
}

struct Fixture {
    _tmp: tempfile::TempDir,
    base: PathBuf,
    repo: PathBuf,
    other: PathBuf,
    gh_dir: PathBuf,
    marker: PathBuf,
}

impl Fixture {
    /// `repo` clones a bare remote; `other` pushes upstream commits for Pull. `repo`'s own config then names a
    /// program for every key, each writing its name to the marker.
    fn new() -> Self {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let base = fs::canonicalize(tmp.path()).expect("canonical");
        let (repo, other, gh_dir) = (base.join("repo"), base.join("other"), base.join("gh"));
        fs::create_dir_all(&gh_dir).expect("gh dir");
        git(&base, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
        git(&base, &["clone", "-q", "remote.git", "other"]);
        for (k, v) in [("user.name", "t"), ("user.email", "t@example.com")] {
            git(&other, &["config", k, v]);
        }
        fs::write(other.join(".gitattributes"), "*.txt filter=ev merge=ev\n").expect("attrs");
        fs::write(other.join("a.txt"), "1\n2\n3\n").expect("write");
        git(&other, &["add", "."]);
        git(&other, &["commit", "-q", "-m", "init"]);
        git(&other, &["push", "-q", "origin", "HEAD:main"]);
        git(&base, &["clone", "-q", "remote.git", "repo"]);
        let marker = base.join("ran");
        let tool = |name: &str| {
            let path = base.join(format!("evil-{name}"));
            script(
                &path,
                &format!("echo {name} >> '{}'\ncat", marker.display()),
            );
            path.to_str().expect("utf-8").to_owned()
        };
        for (k, v) in [
            ("user.name", "t".to_owned()),
            ("user.email", "t@example.com".to_owned()),
            ("filter.ev.clean", tool("clean")),
            ("filter.ev.smudge", tool("smudge")),
            ("filter.ev.process", tool("process")),
            ("merge.ev.driver", tool("merge")),
            ("core.fsmonitor", tool("fsmonitor")),
            ("core.sshCommand", tool("ssh")),
            ("core.askPass", tool("askpass")),
            ("credential.helper", format!("!{}", tool("credential"))),
            ("gpg.program", tool("gpg")),
            ("gpg.ssh.program", tool("gpg-ssh")),
            ("gpg.x509.program", tool("gpg-x509")),
            ("gpg.ssh.defaultKeyCommand", tool("ssh-default-key")),
            // A hook set in config (git 2.5x), which `core.hooksPath` doesn't cover.
            ("hook.cfg.command", tool("config-hook")),
            ("hook.cfg.event", "reference-transaction".to_owned()),
        ] {
            git(&repo, &["config", k, &v]);
        }
        Fixture {
            _tmp: tmp,
            base,
            repo,
            other,
            gh_dir,
            marker,
        }
    }

    fn gh(&self, bin: &Path) -> GitHub<'_> {
        let bin: &'static Path = Box::leak(bin.to_path_buf().into_boxed_path());
        GitHub {
            gh: bin,
            config_dir: &self.gh_dir,
        }
    }

    fn fake_gh(&self) -> GitHub<'_> {
        self.gh(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh"))
    }

    fn ran(&self) -> String {
        fs::read_to_string(&self.marker).unwrap_or_default()
    }

    /// Someone else pushes a change to a.txt, for Pull to fast-forward.
    fn upstream_moves(&self) {
        git(&self.other, &["pull", "-q", "--ff-only"]);
        fs::write(self.other.join("a.txt"), "1\n2\nB\n").expect("write");
        git(&self.other, &["commit", "-q", "-am", "upstream"]);
        git(&self.other, &["push", "-q", "origin", "HEAD:main"]);
    }
}

#[test]
fn background_calls_never_run_the_repos_programs() {
    let f = Fixture::new();
    let repo = &f.repo;
    // A branch whose a.txt differs, for the switch below to check out (smudge) after stashing (clean).
    git(repo, &["switch", "-q", "-c", "side"]);
    fs::write(repo.join("a.txt"), "X\n2\n3\n").expect("write");
    git(repo, &["commit", "-q", "-am", "side"]);
    git(repo, &["switch", "-q", "main"]);
    let _ = fs::remove_file(&f.marker); // the test's own plain git ran them
    fs::write(repo.join("a.txt"), "A\n2\n3\n").expect("write");
    fs::write(repo.join("b.txt"), "new\n").expect("write");
    skyelab_git::status(repo).expect("status");
    skyelab_git::diff(repo, "HEAD").expect("diff");
    skyelab_git::diff_stat(repo, None).expect("diff stat");
    skyelab_git::snapshot_tree(repo).expect("snapshot");
    skyelab_git::capture_checkpoint(repo, "t1", 1).expect("checkpoint");
    skyelab_git::fetch(repo, &f.fake_gh()).expect("fetch");
    // A branch switch with a stash. (No SkyeLab call merges content today; merge drivers are covered by the unit test.)
    fs::write(repo.join("a.txt"), "1\n2\nS\n").expect("write");
    skyelab_git::switch_branch(repo, "side", skyelab_git::OnDirty::Stash).expect("switch");
    assert_eq!(f.ran(), "", "a repo-config program ran");
}

#[test]
fn hooks_off_commit_push_pull_never_run_the_repos_programs() {
    let f = Fixture::new();
    let repo = &f.repo;
    fs::write(repo.join("a.txt"), "A\n2\n3\n").expect("write");
    let tree = skyelab_git::snapshot_tree(repo).expect("snapshot");
    skyelab_git::commit(repo, "mine", &[], &[], false, Some(&tree)).expect("commit");
    skyelab_git::push_with(repo, &f.fake_gh(), &[], false).expect("push");
    f.upstream_moves();
    // Pull refuses a diverged branch, so drop ours first: the fast-forward checks a.txt out (smudge).
    assert_eq!(f.ran(), "", "a repo-config program ran");
    git(repo, &["reset", "-q", "--hard", "origin/main~1"]);
    let _ = fs::remove_file(&f.marker); // the test's own plain git ran them
    let pulled = skyelab_git::pull_ff(repo, &f.fake_gh(), &[], false).expect("pull");
    assert!(pulled.starts_with("Pulled"), "{pulled}");
    assert_eq!(f.ran(), "", "a repo-config program ran");
    // Signing asked for by the repo's config uses git's own programs, never the repo's (which commit fails here
    // doesn't matter: no key is set up).
    for (k, v) in [
        ("commit.gpgSign", "true"),
        ("gpg.format", "ssh"),
        ("user.signingKey", ""),
    ] {
        git(repo, &["config", k, v]);
    }
    fs::write(repo.join("a.txt"), "signed\n").expect("write");
    let _ = skyelab_git::commit(repo, "signed", &[], &[], false, None);
    git(repo, &["config", "gpg.format", "x509"]);
    let _ = skyelab_git::commit(repo, "signed", &[], &[], false, None);
    git(repo, &["config", "gpg.format", "openpgp"]);
    let _ = skyelab_git::commit(repo, "signed", &[], &[], false, None);
    assert_eq!(f.ran(), "", "a repo-config signing program ran");
}

#[test]
fn a_required_repo_filter_fails_closed_instead_of_committing_unfiltered() {
    let f = Fixture::new();
    git(&f.repo, &["config", "filter.ev.required", "true"]);
    fs::write(f.repo.join("a.txt"), "secret\n").expect("write");
    let err = skyelab_git::snapshot_tree(&f.repo).expect_err("a required filter can't be skipped");
    assert!(err.to_string().contains("filter"), "{err}");
    assert_eq!(f.ran(), "", "the filter ran");
}

#[test]
fn a_driver_name_that_cant_be_overridden_is_refused() {
    let f = Fixture::new();
    git(&f.repo, &["config", "filter.a=b.clean", "x"]);
    let err = skyelab_git::status(&f.repo).expect_err("refused");
    assert!(err.to_string().contains("a=b"), "{err}");
}

#[test]
fn gh_pr_create_runs_its_git_without_the_repos_programs() {
    let f = Fixture::new();
    fs::write(f.repo.join("a.txt"), "changed\n").expect("write");
    // gh's own `git status` (it warns about uncommitted changes) is where a filter or fsmonitor would run.
    let gh = f.base.join("gh-runs-git");
    script(
        &gh,
        "cat > /dev/null\ngit status --porcelain > /dev/null || exit 1\necho https://github.com/o/r/pull/1",
    );
    let url = skyelab_git::create_pr(&f.repo, &f.gh(&gh), "t", "b").expect("create pr");
    assert_eq!(url, "https://github.com/o/r/pull/1");
    assert_eq!(f.ran(), "", "a repo-config program ran under gh");
}
