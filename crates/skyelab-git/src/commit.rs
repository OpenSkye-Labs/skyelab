//! Commit, only what the user reviewed, and the working-tree snapshot that review is checked against.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::publish::{MAX_MESSAGE, checked, invalid};
use crate::{Result, command, git, git_succeeds, lossy, repo_root};

/// Commits `paths` (every change, untracked files included, when empty) with `message`, and returns the new commit.
/// With `hooks`, the repo's hooks run, with the scrubbed env plus `env` (the bound account's launch env and git
/// credential binding); without, they are off.
///
/// With `reviewed`, the [`snapshot_tree`] the user looked at: nothing is committed unless the working tree still
/// matches it. The commit is then built from that tree's blobs in a private index, never from the files again, so a
/// write after the check can't get in, and the real index is touched only once the commit exists (a failed commit
/// leaves the user's staging as it was). Ticked `paths` are HEAD plus those paths as reviewed: other staged files stay
/// out of the commit and staged. Either way, a commit that touches a path outside `paths` (or, committing everything,
/// outside what changed between `HEAD` and the reviewed tree) is undone: a pre-commit hook, which an agent can write,
/// may `git add` anything.
/// `dir` relative to the repo's root, when it is a subfolder of it (`None` at the root itself).
fn folder_in_repo(dir: &Path, root: &Path) -> Option<PathBuf> {
    let (dir, root) = (dir.canonicalize().ok()?, root.canonicalize().ok()?);
    let rel = dir.strip_prefix(&root).ok()?;
    (!rel.as_os_str().is_empty()).then(|| rel.to_owned())
}

pub fn commit(
    dir: &Path,
    message: &str,
    paths: &[PathBuf],
    env: &[(OsString, OsString)],
    hooks: bool,
    reviewed: Option<&str>,
) -> Result<String> {
    if message.trim().is_empty() || message.contains('\0') || message.len() > MAX_MESSAGE {
        return Err(invalid(
            "a commit message must be non-empty text under 64 KiB",
        ));
    }
    let root = repo_root(dir)?;
    // Commit all from a project inside a bigger repo commits that folder only, never the rest of the repo.
    let folder: Vec<PathBuf>;
    let paths = match (paths.is_empty(), folder_in_repo(dir, &root)) {
        (true, Some(rel)) => {
            folder = vec![rel];
            &folder[..]
        }
        _ => paths,
    };
    let tree = snapshot_tree(&root)?;
    if reviewed.is_some_and(|r| r != tree) {
        return Err(invalid(
            "the files changed since you looked; nothing was committed. Check the Changes tab again, then commit",
        ));
    }
    if !paths.is_empty()
        && let Some(op) = in_progress(&root)?
    {
        return Err(invalid(format!(
            "a {op} is in progress: commit all files to conclude it, or finish it first"
        )));
    }
    let before = head(&root)?;
    // `sub/` (an untracked nested repo in status) is the gitlink `sub`.
    let names: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().trim_end_matches('/').to_owned())
        .collect();
    let scratch = Scratch::new()?;
    let index = scratch.index();
    let allowed = if paths.is_empty() {
        // A copy of the real index keeps its stat cache, so unchanged files aren't hashed again.
        copy_index(&root, &index)?;
        let tree: &str = &tree;
        if indexed(&root, &index, &["read-tree", "-m", tree], None).is_err() {
            indexed(&root, &index, &["read-tree", tree], None)?; // -m refuses an index with conflicts
        }
        changed_paths(&root, before.as_deref(), tree)?
    } else {
        if let Some(before) = &before {
            indexed(&root, &index, &["read-tree", before], None)?;
        }
        let under = |cmd: &[&str]| -> Vec<String> {
            let mut args: Vec<String> = ["--literal-pathspecs"]
                .iter()
                .chain(cmd)
                .map(|a| a.to_string())
                .collect();
            args.push("--".into());
            args.extend(names.iter().cloned());
            args
        };
        // A staged deletion or a rename's old path is gone from the reviewed tree: removing it records that.
        let stale = indexed(&root, &index, &under(&["ls-files", "-z"]), None)?;
        let wanted = indexed(
            &root,
            &index,
            &under(&["ls-tree", "-r", "-z", "--full-tree", &tree]),
            None,
        )?;
        if stale.is_empty() && wanted.is_empty() {
            return Err(invalid(
                "nothing to commit: none of these files exists any more",
            ));
        }
        if !stale.is_empty() {
            let remove = ["update-index", "-z", "--force-remove", "--stdin"];
            indexed(&root, &index, &remove, Some(&stale))?;
        }
        if !wanted.is_empty() {
            let add = ["update-index", "-z", "--index-info"];
            indexed(&root, &index, &add, Some(&wanted))?;
        }
        names.clone()
    };
    let mut cmd = crate::user_command(&root, hooks);
    cmd.args(["commit", "-F", "-"])
        .env("GIT_INDEX_FILE", &index)
        .envs(env.iter().map(|(k, v)| (k, v)));
    checked(cmd, "git commit", Some(message.as_bytes()))?;
    let sha = lossy(&git(&root, &["rev-parse", "HEAD"])?)
        .trim()
        .to_owned();
    let undo = |why: String| -> Result<String> {
        match &before {
            Some(b) => git(&root, &["reset", "-q", "--soft", b])?,
            None => git(&root, &["update-ref", "-d", "HEAD"])?,
        };
        Err(invalid(why))
    };
    // Built on `before`: a commit another client made in between would be reverted by ours.
    let parent = crate::output(
        Path::new("git"),
        &root,
        &["rev-parse", "-q", "--verify", &format!("{sha}^")],
    )?;
    let parent = parent
        .status
        .success()
        .then(|| lossy(&parent.stdout).trim().to_owned());
    if parent != before {
        if let Some(parent) = &parent {
            git(&root, &["reset", "-q", "--soft", parent])?;
        }
        return Err(invalid(
            "another commit landed while this one was made; nothing was committed. Check the Changes tab again",
        ));
    }
    let extra: Vec<String> = changed_paths(&root, before.as_deref(), &sha)?
        .into_iter()
        .filter(|p| {
            !allowed
                .iter()
                .any(|a| p == a || p.starts_with(&format!("{a}/")))
        })
        .collect();
    if !extra.is_empty() {
        return undo(format!(
            "a commit hook added files you didn't review ({}); the commit was undone",
            extra.join(", ")
        ));
    }
    // The real index catches up with the commit. The commit exists either way, so a failure here (an index.lock
    // someone holds) is not a failed commit: the next status shows what is staged.
    if let Err(e) = catch_up_index(&root, &names, paths.is_empty()) {
        eprintln!("skyelab: committed {sha}, but the index wasn't updated: {e}");
    }
    Ok(sha)
}

/// Sets the real index to HEAD: all of it (`read-tree`, which keeps ORIG_HEAD, unlike `reset`), or the ticked paths.
fn catch_up_index(root: &Path, names: &[String], all: bool) -> Result<()> {
    if all {
        if git(root, &["read-tree", "-m", "HEAD"]).is_err() {
            git(root, &["read-tree", "HEAD"])?; // -m refuses an index with conflicts
        }
        return Ok(());
    }
    let mut reset = vec![
        "--literal-pathspecs".to_owned(),
        "reset".into(),
        "-q".into(),
        "--".into(),
    ];
    reset.extend(names.iter().cloned());
    git(root, &reset).map(drop)
}

/// The operation a partial commit would break: git refuses `commit -- <paths>` during a merge, cherry-pick or rebase,
/// and a revert in progress gets the same caution.
fn in_progress(root: &Path) -> Result<Option<&'static str>> {
    for (head, op) in [
        ("MERGE_HEAD", "merge"),
        ("CHERRY_PICK_HEAD", "cherry-pick"),
        ("REVERT_HEAD", "revert"),
    ] {
        if git_succeeds(root, &["rev-parse", "-q", "--verify", head])? {
            return Ok(Some(op));
        }
    }
    for dir in ["rebase-merge", "rebase-apply"] {
        let path = lossy(&git(
            root,
            &["rev-parse", "--path-format=absolute", "--git-path", dir],
        )?);
        if Path::new(path.trim_end_matches('\n')).exists() {
            return Ok(Some("rebase"));
        }
    }
    Ok(None)
}

/// git on `root` with hooks off and `index` as its index.
pub(crate) fn indexed<S: AsRef<OsStr>>(
    root: &Path,
    index: &Path,
    args: &[S],
    input: Option<&[u8]>,
) -> Result<Vec<u8>> {
    let mut cmd = command(Path::new("git"), root);
    cmd.args(args).env("GIT_INDEX_FILE", index);
    crate::finish(root, args, skyelab_core::process::output(&mut cmd, input)?)
}

fn copy_index(root: &Path, to: &Path) -> Result<()> {
    let index = PathBuf::from(
        lossy(&git(
            root,
            &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        )?)
        .trim_end_matches('\n'),
    );
    match std::fs::copy(&index, to) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// A fresh private dir in the temp dir (never in `.git`, which an agent can write) for a scratch index, removed on drop.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new() -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "skyelab-snapshot-{}-{n}-{nanos}",
            std::process::id()
        ));
        // Exclusive: fails rather than reuse a dir someone else made.
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut std::fs::DirBuilder::new(), 0o700)
            .create(&dir)?;
        #[cfg(not(unix))]
        std::fs::create_dir(&dir)?;
        Ok(Self(dir))
    }

    pub(crate) fn index(&self) -> PathBuf {
        self.0.join("index")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `HEAD`'s commit, `None` in a repo with no commits yet.
fn head(root: &Path) -> Result<Option<String>> {
    let out = crate::output(
        Path::new("git"),
        root,
        &["rev-parse", "--verify", "-q", "HEAD"],
    )?;
    Ok(out
        .status
        .success()
        .then(|| lossy(&out.stdout).trim().to_owned()))
}

/// The paths that differ between commit (or tree) `from` (`None`: nothing) and `to`.
fn changed_paths(root: &Path, from: Option<&str>, to: &str) -> Result<Vec<String>> {
    let out = match from {
        Some(from) => git(
            root,
            &[
                "diff-tree",
                "-r",
                "-z",
                "--name-only",
                "--no-renames",
                from,
                to,
            ],
        )?,
        None => git(
            root,
            &["ls-tree", "-r", "-z", "--name-only", "--full-tree", to],
        )?,
    };
    Ok(lossy(&out)
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect())
}

/// The tree id of everything the working tree of `dir`'s repo holds now (untracked files included, ignored ones
/// not), as a commit of all of it would record. Built in a scratch index in a fresh private dir (never in `.git`,
/// which an agent can write), so the real index is untouched.
pub fn snapshot_tree(dir: &Path) -> Result<String> {
    let root = repo_root(dir)?;
    let scratch = Scratch::new()?;
    let index = scratch.index();
    copy_index(&root, &index)?;
    indexed(&root, &index, &["add", "-A"], None)?;
    let tree = indexed(&root, &index, &["write-tree"], None)?;
    Ok(lossy(&tree).trim().to_owned())
}
