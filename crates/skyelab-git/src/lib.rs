//! Git for SkyeLab: a worktree per agent thread, plus status and structured diffs.
//!
//! shortcut: shells out to the `git` CLI, so every call pays a process spawn and we parse text output;
//! move to `gix` once it covers worktree add/remove and status with renames.

mod branch;
mod checkpoint;
mod commit;
mod diff;
mod gh_lists;
mod pr_actions;
mod publish;
mod remote;
mod status;
mod summary;
pub mod watch;
mod worktree;

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub use branch::{Branches, OnDirty, branches, create_branch, switch_branch};
pub use checkpoint::{
    Checkpoint, RewindPlan, capture_checkpoint, checkpoints, delete_checkpoints, plan_rewind,
    rewind,
};
pub use commit::{commit, snapshot_tree};
pub use diff::{
    Against, FileDiff, Hunk, Line, MAX_FILE_LINES, MAX_TOTAL_LINES, diff, diff_merge_base,
    diff_stat, diff_trees, diff_with,
};
pub use gh_lists::{
    IssueFilter, IssueRow, MAX_BODY, MAX_ROWS, PrFilter, PrRow, issue_list_args, list_issues,
    list_prs, parse_issue_list, parse_pr_list, parse_repo_url, pr_list_args, repo_slug,
};
pub use pr_actions::{MergeMethod, Verdict, merge_methods, merge_pr, review_pr};
pub use publish::{GitHub, create_pr, credential_args, gh_login, push, push_with};
pub use remote::{
    Checks, FailedCheck, MAX_FAILED_CHECKS, PrState, PrStatus, Review, ahead_behind, fetch,
    gh_logged_in, parse_pr_view, pr_status, pull_ff,
};
pub use status::{FileChange, Status, status, status_with_ignored};
pub use summary::{MAX_DIFF, PrContext, TRUNCATED, commit_diff, pr_context};
pub use worktree::{Worktree, create_worktree, list_worktrees, remove_worktree};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("git is not installed (no `git` on PATH)")]
    GitNotInstalled,
    #[error("{} is not inside a git repository", .0.display())]
    NotARepo(PathBuf),
    #[error("branch `{0}` already exists")]
    BranchExists(String),
    #[error("`{0}` is not a valid branch name")]
    InvalidBranch(String),
    #[error("worktree {} has uncommitted, untracked or ignored files", .0.display())]
    Dirty(PathBuf),
    #[error("no platform data directory to put worktrees in")]
    NoDataDir,
    #[error("{0}")]
    Invalid(String),
    #[error("gh is not installed (no `gh` on PATH)")]
    GhNotInstalled,
    #[error("`{args}` failed: {stderr}")]
    Gh { args: String, stderr: String },
    #[error("`git {args}` failed: {stderr}")]
    Git { args: String, stderr: String },
    #[error("running git: {0}")]
    Io(#[from] io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// How a file changed, shared by [`status`] and [`diff`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Unmerged,
}

/// The top-level directory of the repo (or worktree) containing `path`.
pub fn repo_root(path: &Path) -> Result<PathBuf> {
    let out = git(path, &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(lossy(&out).trim_end_matches('\n')))
}

/// The files git tracks under `dir`, relative to it: what the composer's `@` menu offers. Ignored and untracked
/// files never show.
pub fn tracked_files(dir: &Path) -> Result<Vec<String>> {
    ls_files(dir, &["--cached"])
}

/// What the file picker lists: the tracked files under `dir` plus the untracked ones that aren't ignored, relative to
/// it and sorted.
pub fn project_files(dir: &Path) -> Result<Vec<String>> {
    let mut files = ls_files(dir, &["--cached", "--others", "--exclude-standard"])?;
    files.sort();
    files.dedup(); // an unmerged file is listed once per stage
    files.retain(|f| !f.ends_with('/')); // an untracked nested repo is listed as `sub/`
    Ok(files)
}

fn ls_files(dir: &Path, which: &[&str]) -> Result<Vec<String>> {
    let mut args = vec!["-c", "core.quotePath=false", "ls-files", "-z"];
    args.extend_from_slice(which);
    let out = git(dir, &args)?;
    Ok(out
        .split(|b| *b == 0)
        .filter(|f| !f.is_empty())
        .map(lossy)
        .collect())
}

/// The branch checked out in `dir`, or `None` on a detached HEAD. Not `--short`: with a tag of the same name it
/// answers `heads/<branch>`.
pub fn current_branch(dir: &Path) -> Result<Option<String>> {
    let args = ["symbolic-ref", "--quiet", "HEAD"];
    let out = output(Path::new("git"), dir, &args)?;
    if out.status.code() == Some(1) && out.stderr.is_empty() {
        return Ok(None);
    }
    let out = finish(dir, &args, out)?;
    let full = lossy(&out);
    Ok(full.trim().strip_prefix("refs/heads/").map(str::to_owned))
}

/// The full SHA of commit-ish `rev` in the repo at `dir`.
pub fn rev_parse(dir: &Path, rev: &str) -> Result<String> {
    let out = git(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{rev}^{{commit}}"),
        ],
    )?;
    Ok(lossy(&out).trim().to_owned())
}

/// Adds `pattern` to the repo's own `info/exclude` (shared by all its worktrees, never committed), once.
pub fn exclude_locally(repo: &Path, pattern: &str) -> Result<()> {
    let out = git(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let file = PathBuf::from(lossy(&out).trim_end_matches('\n'))
        .join("info")
        .join("exclude");
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    if text.lines().any(|l| l.trim() == pattern) {
        return Ok(());
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let sep = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&file)?;
    io::Write::write_all(&mut f, format!("{sep}{pattern}\n").as_bytes())?;
    Ok(())
}

fn git<S: AsRef<OsStr>>(dir: &Path, args: &[S]) -> Result<Vec<u8>> {
    git_with(Path::new("git"), dir, args)
}

fn git_with<S: AsRef<OsStr>>(bin: &Path, dir: &Path, args: &[S]) -> Result<Vec<u8>> {
    finish(dir, args, output(bin, dir, args)?)
}

/// `out`'s stdout, or the error its exit status and stderr name.
fn finish<S: AsRef<OsStr>>(dir: &Path, args: &[S], out: Output) -> Result<Vec<u8>> {
    if out.status.success() {
        return Ok(out.stdout);
    }
    let stderr = lossy(&out.stderr).trim().to_owned();
    if stderr.contains("not a git repository") {
        return Err(Error::NotARepo(dir.to_owned()));
    }
    let args = args
        .iter()
        .map(|a| a.as_ref().to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    Err(Error::Git { args, stderr })
}

/// For questions git answers with its exit status (does this ref exist?).
fn git_succeeds(dir: &Path, args: &[&str]) -> Result<bool> {
    Ok(output(Path::new("git"), dir, args)?.status.success())
}

fn output<S: AsRef<OsStr>>(bin: &Path, dir: &Path, args: &[S]) -> Result<Output> {
    skyelab_core::process::output(command(bin, dir).args(args), None).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            Error::GitNotInstalled
        } else {
            Error::Io(e)
        }
    })
}

/// Git never through a shell, always on `dir`, with repo hooks off: what SkyeLab only reads (status, diff) or sets
/// up (worktrees) must never run code an agent wrote into `.git/hooks` or `core.hooksPath`.
fn command(bin: &Path, dir: &Path) -> Command {
    let mut cmd = hooked_command(bin, dir);
    cmd.args(["-c", "core.hooksPath=/dev/null"]);
    cmd
}

/// [`command`] with the repo's hooks on, for Commit and Push (users rely on pre-commit and pre-push). Either way
/// git, and any hook or filter it runs, gets only the agents' env allowlist: nothing of SkyeLab's own env (the
/// parent's credentials, CLAUDE_CONFIG_DIR, GH_*, GIT_* that would point git at another repo). The caller adds
/// the bound account's env where one applies.
fn hooked_command(bin: &Path, dir: &Path) -> Command {
    let mut cmd = Command::new(bin);
    cmd.arg("-C").arg(dir);
    // A repo's config can name a program for core.fsmonitor that every status would run.
    cmd.args(["-c", "core.fsmonitor=false"]);
    cmd.env_clear()
        .envs(skyelab_core::env::inherited_agent_env(false));
    // English messages so the "not a git repository" match holds in any locale.
    cmd.env("LC_ALL", "C");
    // Read-only calls like status must not take index.lock while an agent is committing.
    cmd.env("GIT_OPTIONAL_LOCKS", "0");
    cmd
}

/// A CA file the child git trusts for https (`GIT_SSL_CAINFO`) on push and fetch: the fake https remote's, set by
/// tests and by `SKYELAB_GIT_CA_FILE` in debug runs. Only [`trust_test_ca`] sets it, and only debug builds have that.
/// It takes the place of git's CA file (a curl with a system CA folder still trusts that too); the CA's key never
/// leaves the fake remote's memory, so nothing else can present a cert it signed.
static TEST_CA: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Debug builds only: push and fetch trust `file`'s CA from now on (the first call wins).
#[cfg(debug_assertions)]
pub fn trust_test_ca(file: &Path) {
    let _ = TEST_CA.set(file.to_owned());
}

/// The env a push or fetch adds for [`TEST_CA`].
fn test_ca_env() -> Option<(&'static str, &'static Path)> {
    TEST_CA.get().map(|ca| ("GIT_SSL_CAINFO", ca.as_path()))
}

// shortcut: non-UTF-8 path bytes become U+FFFD; switch to OsStr-from-bytes if a user hits it.
fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod whitespace_test;
