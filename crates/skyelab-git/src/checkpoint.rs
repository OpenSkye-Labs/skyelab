//! Per-turn checkpoints: the working tree after each agent turn, kept as commits under SkyeLab's own ref
//! namespace (`refs/skyelab/checkpoints/<thread key>/<turn>`). Capturing never touches the user's branches,
//! index or `HEAD`; the refs keep the snapshots safe from `git gc` until [`delete_checkpoints`] drops them.
//! Rewinding writes the working tree back to a checkpoint, never through a symlink and never over an ignored file
//! unless the caller says so.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use crate::commit::{Scratch, indexed};
use crate::{Error, Result, git, lossy, repo_root, snapshot_tree};

const NAMESPACE: &str = "refs/skyelab/checkpoints";

/// One captured checkpoint. Turn 0 is the tree before the thread's first turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    pub turn: u32,
    /// The working tree's tree id (as [`snapshot_tree`] records it).
    pub tree: String,
}

/// `key` names one thread; it must be a single ref component (letters, digits, `-`, `_`).
fn prefix(key: &str) -> Result<String> {
    let ok = !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !ok {
        return Err(Error::Invalid(format!("`{key}` is not a checkpoint key")));
    }
    Ok(format!("{NAMESPACE}/{key}/"))
}

/// Records the working tree of `dir`'s repo as checkpoint `turn` of thread `key` (replacing one of that number),
/// and returns its tree id.
pub fn capture_checkpoint(dir: &Path, key: &str, turn: u32) -> Result<String> {
    let name = format!("{}{turn}", prefix(key)?);
    let root = repo_root(dir)?;
    let tree = snapshot_tree(&root)?;
    let mut cmd = crate::command(Path::new("git"), &root);
    // Fixed identity: a repo without user.name must still checkpoint, and nothing here is the user's commit.
    for (k, v) in [
        ("GIT_AUTHOR_NAME", "SkyeLab"),
        ("GIT_AUTHOR_EMAIL", "checkpoint@skyelab.invalid"),
        ("GIT_COMMITTER_NAME", "SkyeLab"),
        ("GIT_COMMITTER_EMAIL", "checkpoint@skyelab.invalid"),
    ] {
        cmd.env(k, v);
    }
    let args = [
        "-c",
        "commit.gpgSign=false",
        "commit-tree",
        "-m",
        "skyelab checkpoint",
        &tree,
    ];
    cmd.args(args);
    let out = crate::finish(&root, &args, skyelab_core::process::output(&mut cmd, None)?)?;
    let commit = lossy(&out).trim().to_owned();
    git(&root, &["update-ref", &name, &commit])?;
    Ok(tree)
}

/// Thread `key`'s checkpoints, by turn.
pub fn checkpoints(dir: &Path, key: &str) -> Result<Vec<Checkpoint>> {
    let prefix = prefix(key)?;
    let out = git(
        dir,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)^{tree}",
            &prefix,
        ],
    )?;
    let mut list = Vec::new();
    for line in lossy(&out).lines() {
        let Some((name, commit)) = line.split_once(' ') else {
            continue;
        };
        let Some(turn) = name.strip_prefix(&prefix).and_then(|t| t.parse().ok()) else {
            continue;
        };
        list.push((turn, commit.to_owned()));
    }
    list.sort();
    if list.is_empty() {
        return Ok(Vec::new());
    }
    // One rev-parse for every `<commit>^{tree}`.
    let revs: Vec<&str> = list.iter().map(|(_, c)| c.as_str()).collect();
    let trees = git(dir, &[&["rev-parse"][..], &revs].concat())?;
    Ok(list
        .into_iter()
        .zip(lossy(&trees).lines().map(str::to_owned))
        .map(|((turn, _), tree)| Checkpoint { turn, tree })
        .collect())
}

/// Deletes every checkpoint ref of thread `key`. The snapshots become unreachable, so `git gc` prunes them.
pub fn delete_checkpoints(dir: &Path, key: &str) -> Result<()> {
    let prefix = prefix(key)?;
    let out = git(dir, &["for-each-ref", "--format=%(refname)", &prefix])?;
    let input: String = lossy(&out)
        .lines()
        .filter(|l| l.starts_with(&prefix))
        .map(|l| format!("delete {l}\n"))
        .collect();
    if input.is_empty() {
        return Ok(());
    }
    let root = repo_root(dir)?;
    let mut cmd = crate::command(Path::new("git"), &root);
    let args = ["update-ref", "--stdin"];
    cmd.args(args);
    crate::finish(
        &root,
        &args,
        skyelab_core::process::output(&mut cmd, Some(input.as_bytes()))?,
    )
    .map(drop)
}

/// What [`rewind`] would change to bring the working tree back to a checkpoint. Paths are relative to the repo root.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RewindPlan {
    /// The checkpoint's tree.
    pub to: String,
    /// The working tree when the plan was made: [`rewind`] refuses if it has changed since.
    pub now: String,
    /// Files written back to their checkpoint content (changed or deleted since).
    pub restore: Vec<PathBuf>,
    /// Files created since, deleted.
    pub remove: Vec<PathBuf>,
    /// Files the checkpoint has that now exist as ignored (or otherwise unrecorded) files: overwritten only when
    /// the caller passes `overwrite_ignored`.
    pub ignored: Vec<PathBuf>,
}

impl RewindPlan {
    pub fn is_empty(&self) -> bool {
        self.restore.is_empty() && self.remove.is_empty() && self.ignored.is_empty()
    }
}

/// What rewinding `dir`'s repo to tree `to` would change.
pub fn plan_rewind(dir: &Path, to: &str) -> Result<RewindPlan> {
    let root = repo_root(dir)?;
    let now = snapshot_tree(&root)?;
    let out = git(
        &root,
        &[
            "diff-tree",
            "-r",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            &now,
            to,
        ],
    )?;
    let mut plan = RewindPlan {
        to: to.to_owned(),
        now,
        ..RewindPlan::default()
    };
    // `:<old mode> <new mode> <old id> <new id> <status>` NUL `<path>` NUL, old = now, new = the checkpoint.
    let mut fields = out.split(|b| *b == 0);
    while let (Some(meta), Some(path)) = (fields.next(), fields.next()) {
        let meta = lossy(meta);
        let mut parts = meta.trim_start_matches(':').split(' ');
        let (Some(old_mode), Some(new_mode)) = (parts.next(), parts.next()) else {
            continue;
        };
        // A submodule's checkout is its own repo: never rewound from here.
        if old_mode == "160000" || new_mode == "160000" {
            continue;
        }
        let path = PathBuf::from(lossy(path));
        safe_relative(&path)?;
        let status = parts.nth(2).unwrap_or("");
        match status {
            "D" => plan.remove.push(path),
            "A" if std::fs::symlink_metadata(root.join(&path)).is_ok() => plan.ignored.push(path),
            _ => plan.restore.push(path),
        }
    }
    Ok(plan)
}

/// Brings the working tree back to `plan.to`: deletes what was created since, writes back what changed. The user's
/// index and branches are untouched (rewound files then show as changes against them). Refuses if the working tree
/// changed after the plan was made, and never writes through a symlinked folder, so nothing outside the repo is hit.
pub fn rewind(dir: &Path, plan: &RewindPlan, overwrite_ignored: bool) -> Result<()> {
    let root = repo_root(dir)?;
    if snapshot_tree(&root)? != plan.now {
        return Err(Error::Invalid(
            "the files changed since the rewind was shown; nothing was changed. Look again, then rewind".into(),
        ));
    }
    for path in &plan.remove {
        let full = inside(&root, path)?;
        match std::fs::remove_file(&full) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => prune_empty(&root, &full),
        }
    }
    let mut write: Vec<&PathBuf> = plan.restore.iter().collect();
    if overwrite_ignored {
        write.extend(&plan.ignored);
    }
    if write.is_empty() {
        return Ok(());
    }
    let mut input = Vec::new();
    for path in &write {
        inside(&root, path)?;
        input.extend_from_slice(path.as_os_str().as_encoded_bytes());
        input.push(0);
    }
    // The checkpoint in a scratch index, checked out over just these paths: git writes the files (modes, symlinks,
    // filters) and the real index stays as it was.
    let scratch = Scratch::new()?;
    let index = scratch.index();
    indexed(&root, &index, &["read-tree", &plan.to], None)?;
    indexed(
        &root,
        &index,
        &["checkout-index", "-f", "-z", "--stdin"].map(OsStr::new),
        Some(&input),
    )?;
    Ok(())
}

/// Only plain relative components, and never into `.git`.
fn safe_relative(path: &Path) -> Result<()> {
    let ok = path.components().all(|c| match c {
        Component::Normal(n) => !n.eq_ignore_ascii_case(".git"),
        _ => false,
    });
    if !ok {
        return Err(Error::Invalid(format!(
            "refusing to rewind {}: not a plain path inside the repo",
            path.display()
        )));
    }
    Ok(())
}

/// `root/path`, once no folder on the way there is a symlink (which could lead out of the repo).
fn inside(root: &Path, path: &Path) -> Result<PathBuf> {
    safe_relative(path)?;
    let mut at = root.to_owned();
    let mut parts = path.components().peekable();
    while let Some(part) = parts.next() {
        at.push(part);
        if parts.peek().is_some()
            && std::fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(Error::Invalid(format!(
                "refusing to rewind {}: {} is a symlink",
                path.display(),
                at.display()
            )));
        }
    }
    Ok(at)
}

/// Removes the folders above `file` that its deletion left empty, up to (never including) `root`.
fn prune_empty(root: &Path, file: &Path) {
    let mut dir = file.parent();
    while let Some(d) = dir.filter(|d| *d != root && d.starts_with(root)) {
        if std::fs::remove_dir(d).is_err() {
            break; // not empty (or not ours to remove)
        }
        dir = d.parent();
    }
}
