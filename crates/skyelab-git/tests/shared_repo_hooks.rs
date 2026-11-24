//! Acceptance: Commit, Push and Pull run the repo's hooks only while no other account shares its git dir.
//! A planted hook writes a marker; it runs in a single-account repo and never in a shared one (a worktree bound to
//! another account, a bound subfolder of an unbound repo). Remotes are local bare repos and `gh` is a fake.
#![cfg(unix)] // the hooks and the fake gh are shell scripts

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use skyelab_core::{Account, AccountsConfig, Binding, Provider};
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
    /// Ours: `main` tracking `origin/main`, with every hook planted.
    repo: PathBuf,
    /// Someone else's clone, which pushes new commits for Pull to bring in.
    other: PathBuf,
    gh_dir: PathBuf,
    marker: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let base = fs::canonicalize(tmp.path()).expect("canonical");
        let (repo, other, gh_dir) = (base.join("repo"), base.join("other"), base.join("gh"));
        fs::create_dir_all(&gh_dir).expect("gh dir");
        git(&base, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
        git(&base, &["clone", "-q", "remote.git", "other"]);
        configure(&other);
        fs::write(other.join("a.txt"), "one\n").expect("write");
        git(&other, &["add", "."]);
        git(&other, &["commit", "-q", "-m", "init"]);
        git(&other, &["push", "-q", "origin", "HEAD:main"]);
        git(&base, &["clone", "-q", "remote.git", "repo"]);
        configure(&repo);
        let marker = base.join("hooks-ran");
        let hooks = repo.join(".git/hooks");
        for name in [
            "pre-commit",
            "commit-msg",
            "post-commit",
            "pre-push",
            "post-merge",
        ] {
            let hook = hooks.join(name);
            let script = format!("#!/bin/sh\necho {name} >> '{}'\n", marker.display());
            fs::write(&hook, script).expect("plant hook");
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("chmod");
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

    fn gh(&self) -> GitHub<'_> {
        let bin: &'static Path = Box::leak(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/fake-gh.sh")
                .into_boxed_path(),
        );
        GitHub {
            gh: bin,
            config_dir: &self.gh_dir,
        }
    }

    /// Accounts `a` and `b`, with `bindings` as (folder, account).
    fn config(&self, bindings: &[(&Path, &str)]) -> AccountsConfig {
        let account = |id: &str| Account {
            id: id.into(),
            label: id.into(),
            provider: Provider::default(),
            config_dir: self.base.join(format!("claude-{id}")),
            gh_config_dir: Some(self.gh_dir.clone()),
            ssh_agent: false,
        };
        AccountsConfig {
            accounts: vec![account("a"), account("b")],
            bindings: bindings
                .iter()
                .map(|(p, id)| Binding::new(p.to_path_buf(), (*id).into()))
                .collect(),
        }
    }

    /// What the app decides for `dir`: the folders hooks could be written from, then the accounts check.
    fn hooks(&self, config: &AccountsConfig, dir: &Path) -> bool {
        let folders = skyelab_git::hook_folders(dir).expect("hook folders");
        config.repo_hooks_allowed(dir, &folders)
    }

    /// Someone else pushes a commit to `branch`, for Pull to fast-forward.
    fn upstream_moves(&self, branch: &str) {
        git(&self.other, &["fetch", "-q", "origin"]);
        git(
            &self.other,
            &["checkout", "-q", "-B", branch, &format!("origin/{branch}")],
        );
        fs::write(self.other.join(format!("{branch}.txt")), "new\n").expect("write");
        git(&self.other, &["add", "."]);
        git(&self.other, &["commit", "-q", "-m", "upstream"]);
        git(&self.other, &["push", "-q", "origin", branch]);
    }

    /// Commit, Push and Pull in `dir` with the decided `hooks`; returns which hooks wrote the marker.
    fn commit_push_pull(&self, dir: &Path, hooks: bool) -> String {
        let _ = fs::remove_file(&self.marker);
        fs::write(dir.join("mine.txt"), "mine\n").expect("write");
        skyelab_git::commit(dir, "mine", &[], &[], hooks, None).expect("commit");
        skyelab_git::push_with(dir, &self.gh(), &[], hooks).expect("push");
        let branch = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]);
        self.upstream_moves(&branch);
        let pulled = skyelab_git::pull_ff(dir, &self.gh(), &[], hooks).expect("pull");
        assert!(pulled.starts_with("Pulled 1"), "{pulled}");
        fs::read_to_string(&self.marker).unwrap_or_default()
    }
}

#[test]
fn single_account_repo_runs_its_hooks() {
    let f = Fixture::new();
    let config = f.config(&[(&f.repo, "a")]);
    assert!(f.hooks(&config, &f.repo));
    let ran = f.commit_push_pull(&f.repo, true);
    for hook in [
        "pre-commit",
        "commit-msg",
        "post-commit",
        "pre-push",
        "post-merge",
    ] {
        assert!(ran.contains(hook), "{hook} didn't run: {ran:?}");
    }
}

#[test]
fn a_worktree_bound_to_another_account_never_runs_the_hooks() {
    let f = Fixture::new();
    let wt = f.base.join("wt");
    git(
        &f.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "wt",
            wt.to_str().expect("utf-8"),
        ],
    );
    let config = f.config(&[(&f.repo, "a"), (&wt, "b")]);
    // Both sides see the other account: account a's agent could plant a hook b's Commit runs, and the other way round.
    assert!(!f.hooks(&config, &wt));
    assert!(!f.hooks(&config, &f.repo));
    assert_eq!(f.commit_push_pull(&wt, false), "", "a hook ran");
    // The same worktree on the main checkout's account keeps them.
    let same = f.config(&[(&f.repo, "a"), (&wt, "a")]);
    assert!(f.hooks(&same, &wt));
}

#[test]
fn a_subfolder_project_or_nested_binding_never_runs_the_hooks() {
    let f = Fixture::new();
    let app = f.repo.join("app");
    fs::create_dir_all(&app).expect("mkdir");
    // The repo itself is unbound: whoever can write its .git isn't this account.
    let subfolder = f.config(&[(&app, "a")]);
    assert!(!f.hooks(&subfolder, &app));
    // The repo is bound to a, a folder inside it to b.
    let nested = f.config(&[(&f.repo, "a"), (&app, "b")]);
    assert!(!f.hooks(&nested, &f.repo));
    assert!(!f.hooks(&nested, &app));
    assert_eq!(f.commit_push_pull(&f.repo, false), "", "a hook ran");
}

#[test]
fn a_repo_inside_another_accounts_folder_or_hooks_path_never_runs_the_hooks() {
    let f = Fixture::new();
    // The repo is b's own, but sits inside a's folder: a's agent can write its .git.
    let outer = f.base.clone();
    let nested = f.config(&[(&outer, "a"), (&f.repo, "b")]);
    assert!(!f.hooks(&nested, &f.repo));
    // core.hooksPath pointing into a folder bound to another account (or to none).
    let theirs = f.base.join("theirs");
    fs::create_dir_all(&theirs).expect("mkdir");
    let config = f.config(&[(&f.repo, "a"), (&theirs, "b")]);
    assert!(f.hooks(&config, &f.repo), "the repo alone is a's");
    git(
        &f.repo,
        &["config", "core.hooksPath", theirs.to_str().expect("utf-8")],
    );
    assert!(!f.hooks(&config, &f.repo));
    assert!(!f.hooks(&f.config(&[(&f.repo, "a")]), &f.repo));
    // A separate git dir outside the checkout must be the account's too.
    git(&f.repo, &["config", "--unset", "core.hooksPath"]);
    let moved = f.base.join("moved.git");
    fs::rename(f.repo.join(".git"), &moved).expect("move .git");
    fs::write(
        f.repo.join(".git"),
        format!("gitdir: {}\n", moved.display()),
    )
    .expect(".git file");
    assert!(!f.hooks(&f.config(&[(&f.repo, "a")]), &f.repo));
    let both = f.config(&[(&f.repo, "a"), (&moved, "a")]);
    assert!(f.hooks(&both, &f.repo));
}
