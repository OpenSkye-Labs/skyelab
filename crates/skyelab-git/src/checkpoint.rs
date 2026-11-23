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
    // `%(tree)` is empty for a ref that isn't a commit: such a ref (not ours) is skipped, not fatal.
    let out = git(
        dir,
        &["for-each-ref", "--format=%(refname) %(tree)", &prefix],
    )?;
    let mut list: Vec<Checkpoint> = lossy(&out)
        .lines()
        .filter_map(|line| {
            let (name, tree) = line.split_once(' ')?;
            let turn = name.strip_prefix(&prefix)?.parse().ok()?;
            is_object_id(tree).then(|| Checkpoint {
                turn,
                tree: tree.to_owned(),
            })
        })
        .collect();
    list.sort_by_key(|c| c.turn);
    Ok(list)
}

/// A full hex object id (SHA-1 or SHA-256), so it can't be read as an option or a revision expression.
fn is_object_id(id: &str) -> bool {
    matches!(id.len(), 40 | 64) && id.bytes().all(|b| b.is_ascii_hexdigit())
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
    /// Files of the checkpoint that something git doesn't record (an ignored file or folder) stands in the way of:
    /// written, replacing that, only when the caller passes `overwrite_ignored`.
    pub ignored: Vec<PathBuf>,
}

impl RewindPlan {
    pub fn is_empty(&self) -> bool {
        self.restore.is_empty() && self.remove.is_empty() && self.ignored.is_empty()
    }
}

/// What rewinding `dir` to tree `to` (a checkpoint of its repo) would change. Only paths inside `dir` are rewound, so
/// a project in a subfolder never touches its repo's other folders.
pub fn plan_rewind(dir: &Path, to: &str) -> Result<RewindPlan> {
    if !is_object_id(to) {
        return Err(Error::Invalid(format!("`{to}` is not a tree id")));
    }
    let root = repo_root(dir)?;
    let scope = scope(dir, &root)?;
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
    let mut added = Vec::new();
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
        if !path.starts_with(&scope) {
            continue;
        }
        match parts.nth(2).unwrap_or("") {
            "D" => plan.remove.push(path),
            // In the checkpoint only: whatever stands at its place now decides (after the deletions).
            "A" => added.push(path),
            // In both: a file git records now, so writing it replaces nothing unrecorded.
            _ => plan.restore.push(path),
        }
    }
    let removed = Removed::new(&root, &plan.remove);
    for path in added {
        if blocked(&root, &path, &removed) {
            plan.ignored.push(path);
        } else {
            plan.restore.push(path);
        }
    }
    Ok(plan)
}

/// `dir` relative to its repo's `root` (empty for the root itself).
fn scope(dir: &Path, root: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(dir)?
        .strip_prefix(std::fs::canonicalize(root)?)
        .map(Path::to_owned)
        .map_err(|_| {
            Error::Invalid(format!(
                "{} is not inside its repo {}",
                dir.display(),
                root.display()
            ))
        })
}

/// The paths a rewind deletes. A name that differs only in case counts when it is the same file on disk (a
/// case-insensitive file system, where `Readme.md` and `README.md` are one file).
struct Removed {
    root: PathBuf,
    /// Lowercased path → the path as listed.
    paths: std::collections::HashMap<String, PathBuf>,
}

impl Removed {
    fn new(root: &Path, paths: &[PathBuf]) -> Self {
        let paths = paths
            .iter()
            .map(|p| (p.to_string_lossy().to_lowercase(), p.clone()))
            .collect();
        Self {
            root: root.to_owned(),
            paths,
        }
    }

    fn has(&self, path: &Path) -> bool {
        match self.paths.get(&path.to_string_lossy().to_lowercase()) {
            Some(listed) if listed == path => true,
            Some(listed) => same_entry(&self.root.join(listed), &self.root.join(path)),
            None => false,
        }
    }
}

#[cfg(unix)]
fn same_entry(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    match (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) {
        (Ok(a), Ok(b)) => (a.dev(), a.ino()) == (b.dev(), b.ino()),
        _ => false,
    }
}

/// Windows file systems ignore case.
#[cfg(not(unix))]
fn same_entry(_: &Path, _: &Path) -> bool {
    true
}

/// Whether writing checkpoint file `path` would replace something git doesn't record: a file or symlink (not one
/// the rewind deletes) at `path` or at a folder on the way to it, or a folder at `path` holding anything the rewind
/// doesn't delete.
fn blocked(root: &Path, path: &Path, removed: &Removed) -> bool {
    let mut at = PathBuf::new();
    for part in path.components() {
        at.push(part);
        let Ok(meta) = std::fs::symlink_metadata(root.join(&at)) else {
            return false; // nothing there: free
        };
        if removed.has(&at) {
            return false; // deleted first
        }
        if !meta.is_dir() {
            return true; // an unrecorded file or link in the way
        }
        if at == path {
            return !all_removed(root, &at, removed);
        }
    }
    false
}

/// Every file under folder `dir` is one the rewind deletes (so the folder goes too).
fn all_removed(root: &Path, dir: &Path, removed: &Removed) -> bool {
    let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
        return false;
    };
    entries.flatten().all(|e| {
        let rel = dir.join(e.file_name());
        match e.file_type() {
            Ok(t) if t.is_dir() => all_removed(root, &rel, removed),
            Ok(_) => removed.has(&rel),
            Err(_) => false,
        }
    })
}

/// Brings the working tree back to `plan.to`: deletes what was created since, writes back what changed. The user's
/// index and branches are untouched (rewound files then show as changes against them). Refuses if the working tree
/// changed after the plan was made, and never writes through a symlinked folder, so nothing outside the repo is hit.
pub fn rewind(dir: &Path, plan: &RewindPlan, overwrite_ignored: bool) -> Result<()> {
    if !is_object_id(&plan.to) {
        return Err(Error::Invalid(format!("`{}` is not a tree id", plan.to)));
    }
    let root = repo_root(dir)?;
    // The plan again, now: different files (or an ignored file that appeared in the way) mean the user confirmed
    // something else, so nothing is changed.
    if plan_rewind(dir, &plan.to)? != *plan {
        return Err(Error::Invalid(
            "the files changed since the rewind was shown; nothing was changed. Look again, then rewind".into(),
        ));
    }
    let top = root.join(scope(dir, &root)?);
    let mut write: Vec<&PathBuf> = plan.restore.iter().collect();
    if overwrite_ignored {
        write.extend(&plan.ignored);
    }
    // Every path is checked before anything changes, so a refused rewind changes nothing. A link the rewind deletes
    // first doesn't count; the writes are checked again once it is gone.
    let removed = Removed::new(&root, &plan.remove);
    for path in plan.remove.iter().chain(write.iter().copied()) {
        inside(&root, path, &removed)?;
    }
    let none = Removed::new(&root, &[]);
    for path in &plan.remove {
        let full = inside(&root, path, &none)?;
        match std::fs::remove_file(&full) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => prune_empty(&top, &full),
        }
    }
    if write.is_empty() {
        return Ok(());
    }
    let mut input = Vec::new();
    for path in &write {
        inside(&root, path, &none)?;
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

/// `root/path`, once no folder on the way there is a symlink (which could lead out of the repo), apart from links in
/// `removed` (deleted before anything is written).
fn inside(root: &Path, path: &Path, removed: &Removed) -> Result<PathBuf> {
    safe_relative(path)?;
    let mut at = root.to_owned();
    let mut rel = PathBuf::new();
    let mut parts = path.components().peekable();
    while let Some(part) = parts.next() {
        at.push(part);
        rel.push(part);
        if parts.peek().is_some()
            && !removed.has(&rel)
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

/// Removes the folders above `file` that its deletion left empty, up to (never including) `root` (the thread's folder).
fn prune_empty(root: &Path, file: &Path) {
    let mut dir = file.parent();
    while let Some(d) = dir.filter(|d| *d != root && d.starts_with(root)) {
        if std::fs::remove_dir(d).is_err() {
            break; // not empty (or not ours to remove)
        }
        dir = d.parent();
    }
}
