//! Git for SkyeLab: a worktree per agent thread, plus status and structured diffs.
//!
//! shortcut: shells out to the `git` CLI, so every call pays a process spawn and we parse text output;
//! move to `gix` once it covers worktree add/remove and status with renames.

mod diff;
mod status;
mod worktree;

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub use diff::{FileDiff, Hunk, Line, diff, diff_merge_base};
pub use status::{FileChange, Status, status, status_with_ignored};
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
    let out = output(bin, dir, args)?;
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
    command(bin, dir).args(args).output().map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            Error::GitNotInstalled
        } else {
            Error::Io(e)
        }
    })
}

/// Git never through a shell, always on `dir`, isolated from what the parent process carries.
fn command(bin: &Path, dir: &Path) -> Command {
    let mut cmd = Command::new(bin);
    cmd.arg("-C").arg(dir);
    // A repo's config can name a program for core.fsmonitor that every status would run.
    cmd.args(["-c", "core.fsmonitor=false"]);
    // Set inside git hooks; any of them overrides `-C` and points us at a different repo.
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_PREFIX",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
    ] {
        cmd.env_remove(var);
    }
    // English messages so the "not a git repository" match holds in any locale.
    cmd.env("LC_ALL", "C");
    // Read-only calls like status must not take index.lock while an agent is committing.
    cmd.env("GIT_OPTIONAL_LOCKS", "0");
    cmd
}

// shortcut: non-UTF-8 path bytes become U+FFFD; switch to OsStr-from-bytes if a user hits it.
fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests;
