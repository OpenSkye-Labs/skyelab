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

pub use diff::{FileDiff, Hunk, Line, diff};
pub use status::{FileChange, Status, status};
pub use worktree::{Worktree, create_worktree, list_worktrees, remove_worktree};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("git is not installed (no `git` on PATH)")]
    GitNotInstalled,
    #[error("{} is not inside a git repository", .0.display())]
    NotARepo(PathBuf),
    #[error("branch `{0}` already exists")]
    BranchExists(String),
    #[error("worktree {} has uncommitted or untracked changes", .0.display())]
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

fn git<S: AsRef<OsStr>>(dir: &Path, args: &[S]) -> Result<Vec<u8>> {
    git_with(Path::new("git"), dir, args)
}

/// Runs `git -C dir args…` directly (never through a shell) and returns stdout, or a typed error.
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

/// Spawns git and returns its raw output, whatever the exit status.
fn output<S: AsRef<OsStr>>(bin: &Path, dir: &Path, args: &[S]) -> Result<Output> {
    Command::new(bin)
        .arg("-C")
        .arg(dir)
        .args(args)
        // English messages so the "not a git repository" match holds in any locale.
        .env("LC_ALL", "C")
        // Read-only calls like status must not take index.lock while an agent is committing.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                Error::GitNotInstalled
            } else {
                Error::Io(e)
            }
        })
}

// shortcut: non-UTF-8 path bytes become U+FFFD; switch to OsStr-from-bytes if a user hits it.
fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests;
