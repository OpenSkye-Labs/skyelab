//! Branches of a local checkout: list, create, and switch without losing uncommitted work.

use std::path::Path;

use crate::{Error, Result, git, git_succeeds, lossy, status};

/// The branches of the repo at `dir`, and which one is checked out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Branches {
    /// `None` when `HEAD` is detached.
    pub current: Option<String>,
    /// Local branches, most recently committed first.
    pub local: Vec<String>,
    /// Local branches checked out in another worktree: git won't switch to them here.
    pub elsewhere: Vec<String>,
}

/// What [`switch_branch`] does with uncommitted changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnDirty {
    /// Refuse with [`Error::Dirty`]: the caller asks the user first.
    Refuse,
    /// Stash every change (untracked files included) under a SkyeLab message, then switch.
    Stash,
    /// Take the changes along, as plain `git switch` does; git refuses if one would be overwritten.
    Carry,
}

pub fn branches(dir: &Path) -> Result<Branches> {
    let out = git(
        dir,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname)%00%(worktreepath)",
            "refs/heads/",
        ],
    )?;
    let head = crate::output(Path::new("git"), dir, &["symbolic-ref", "-q", "HEAD"])?;
    let current = head
        .status
        .success()
        .then(|| lossy(&head.stdout))
        .and_then(|s| s.trim().strip_prefix("refs/heads/").map(str::to_owned));
    let (mut local, mut elsewhere) = (Vec::new(), Vec::new());
    for line in lossy(&out).lines() {
        let (name, worktree) = line.split_once('\0').unwrap_or((line, ""));
        let Some(name) = name.strip_prefix("refs/heads/") else {
            continue;
        };
        // The current branch is checked out here; any other with a worktree path is someone else's.
        if !worktree.is_empty() && current.as_deref() != Some(name) {
            elsewhere.push(name.to_owned());
        }
        local.push(name.to_owned());
    }
    Ok(Branches {
        current,
        local,
        elsewhere,
    })
}

/// `name` is a branch name git accepts and can't be read as an option.
fn valid(dir: &Path, name: &str) -> Result<String> {
    let full = format!("refs/heads/{name}");
    if name.is_empty() || name.starts_with('-') || !git_succeeds(dir, &["check-ref-format", &full])?
    {
        return Err(Error::InvalidBranch(name.to_owned()));
    }
    Ok(full)
}

/// Creates `name` at `HEAD` and switches to it. Uncommitted changes come along (nothing in the tree changes).
pub fn create_branch(dir: &Path, name: &str) -> Result<()> {
    let full = valid(dir, name)?;
    if git_succeeds(dir, &["show-ref", "--verify", "--quiet", &full])? {
        return Err(Error::BranchExists(name.to_owned()));
    }
    git(dir, &["switch", "--no-guess", "-c", name]).map(drop)
}

/// Switches the checkout at `dir` to the existing local branch `name`. Ignored files are never overwritten (git
/// refuses instead). With changes, `on_dirty` decides; a failed switch after a stash applies that stash back, so no
/// work is lost.
pub fn switch_branch(dir: &Path, name: &str, on_dirty: OnDirty) -> Result<()> {
    let full = valid(dir, name)?;
    if !git_succeeds(dir, &["show-ref", "--verify", "--quiet", &full])? {
        return Err(Error::Invalid(format!("there is no branch `{name}`")));
    }
    let dirty = !status(dir)?.is_clean();
    let stashed = match (dirty, on_dirty) {
        (false, _) | (true, OnDirty::Carry) => None,
        (true, OnDirty::Refuse) => return Err(Error::Dirty(dir.to_owned())),
        (true, OnDirty::Stash) => {
            let before = stash_top(dir)?;
            let msg = format!("skyelab: changes left on the way to {name}");
            git(dir, &["stash", "push", "--include-untracked", "-m", &msg])?;
            // The entry this push made, by id: the stash stack is shared with other worktrees. `None` when git had
            // nothing it could stash.
            stash_top(dir)?.filter(|top| Some(top) != before.as_ref())
        }
    };
    match git(
        dir,
        &["switch", "--no-guess", "--no-overwrite-ignore", name],
    ) {
        Ok(_) => Ok(()),
        Err(e) => match stashed {
            None => Err(e),
            Some(id) => match git(dir, &["stash", "apply", "--index", &id]) {
                Ok(_) => Err(Error::Invalid(format!(
                    "{e}; your changes are back, and still saved in the stash"
                ))),
                Err(p) => Err(Error::Invalid(format!(
                    "{e}; your changes are in the stash ({id}), restoring them failed: {p}"
                ))),
            },
        },
    }
}

/// The newest stash entry's commit, if any.
fn stash_top(dir: &Path) -> Result<Option<String>> {
    let out = crate::output(
        Path::new("git"),
        dir,
        &["rev-parse", "-q", "--verify", "refs/stash"],
    )?;
    Ok(out
        .status
        .success()
        .then(|| lossy(&out.stdout).trim().to_owned()))
}
