//! Commit, only what the user reviewed, and the working-tree snapshot that review is checked against.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::publish::{MAX_MESSAGE, checked, invalid};
use crate::{Result, command, git, hooked_command, lossy, repo_root};

/// Commits `paths` (every change, untracked files included, when empty) with `message`, and returns the new commit.
/// The repo's hooks run, with the scrubbed env plus `account_env` (the bound account's `launch_env`).
///
/// With `reviewed`, the [`snapshot_tree`] the user looked at: nothing is committed unless the working tree still
/// matches it (for an empty `paths`, the staged tree must equal it exactly). Either way, a commit that touches a path
/// outside `paths` (or, committing everything, outside what changed between `HEAD` and `reviewed`) is undone: a
/// pre-commit hook, which an agent can write, may `git add` anything.
pub fn commit(
    dir: &Path,
    message: &str,
    paths: &[PathBuf],
    account_env: &[(&str, PathBuf)],
    reviewed: Option<&str>,
) -> Result<String> {
    if message.trim().is_empty() || message.contains('\0') || message.len() > MAX_MESSAGE {
        return Err(invalid(
            "a commit message must be non-empty text under 64 KiB",
        ));
    }
    let root = repo_root(dir)?;
    let changed = || {
        invalid(
            "the files changed since you looked; nothing was committed. Check the Changes tab again, then commit",
        )
    };
    if let Some(reviewed) = reviewed
        && snapshot_tree(&root)? != reviewed
    {
        return Err(changed());
    }
    let before = head(&root)?;
    // Literal, so a ticked file named `*` is that file, not a glob.
    let literal: &OsStr = "--literal-pathspecs".as_ref();
    let mut commit: Vec<&OsStr> = vec![literal, "commit".as_ref(), "-F".as_ref(), "-".as_ref()];
    let allowed: Option<Vec<String>> = if paths.is_empty() {
        git(&root, &["add", "-A"])?;
        match reviewed {
            Some(reviewed) => {
                if lossy(&git(&root, &["write-tree"])?).trim() != reviewed {
                    return Err(changed());
                }
                Some(changed_paths(&root, before.as_deref(), reviewed)?)
            }
            None => None,
        }
    } else {
        // A staged deletion or a rename's old path is in neither the index nor the tree, so `add` would refuse
        // it; `commit -- <path>` still records it while `HEAD` has it. A path in none of them (an old rename source
        // in a worktree tab) is left out of both.
        let (on_disk_or_index, in_head) = present(&root, paths, before.as_deref())?;
        let add: Vec<&OsStr> = [literal, "add".as_ref(), "-A".as_ref(), "--".as_ref()]
            .into_iter()
            .chain(on_disk_or_index.iter().map(|p| p.as_os_str()))
            .collect();
        if add.len() > 4 {
            git(&root, &add)?;
        }
        let keep: Vec<&PathBuf> = paths
            .iter()
            .filter(|p| on_disk_or_index.contains(p) || in_head.contains(p))
            .collect();
        if keep.is_empty() {
            return Err(invalid(
                "nothing to commit: none of these files exists any more",
            ));
        }
        commit.push("--".as_ref());
        commit.extend(keep.iter().map(|p| p.as_os_str()));
        // `sub/` (an untracked nested repo in status) is committed as the gitlink `sub`.
        Some(
            paths
                .iter()
                .map(|p| p.to_string_lossy().trim_end_matches('/').to_owned())
                .collect(),
        )
    };
    let mut cmd = hooked_command(Path::new("git"), &root);
    cmd.args(&commit)
        .envs(account_env.iter().map(|(k, v)| (k, v)));
    checked(cmd, "git commit", Some(message.as_bytes()))?;
    let sha = lossy(&git(&root, &["rev-parse", "HEAD"])?)
        .trim()
        .to_owned();
    if let Some(allowed) = allowed {
        let extra: Vec<String> = changed_paths(&root, before.as_deref(), &sha)?
            .into_iter()
            .filter(|p| !allowed.contains(p))
            .collect();
        if !extra.is_empty() {
            match &before {
                Some(b) => git(&root, &["reset", "-q", "--soft", b])?,
                None => git(&root, &["update-ref", "-d", "HEAD"])?,
            };
            return Err(invalid(format!(
                "a commit hook added files you didn't review ({}); the commit was undone",
                extra.join(", ")
            )));
        }
    }
    Ok(sha)
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

/// Which of `paths` are on disk or in the index (so `git add` accepts them), and which are in commit `head`.
fn present<'p>(
    root: &Path,
    paths: &'p [PathBuf],
    head: Option<&str>,
) -> Result<(Vec<&'p PathBuf>, Vec<&'p PathBuf>)> {
    let listed = |cmd: &[&OsStr]| -> Result<Vec<String>> {
        let literal: &OsStr = "--literal-pathspecs".as_ref();
        let args: Vec<&OsStr> = std::iter::once(literal)
            .chain(cmd.iter().copied())
            .chain(std::iter::once("--".as_ref()))
            .chain(paths.iter().map(|p| p.as_os_str()))
            .collect();
        Ok(lossy(&git(root, &args)?)
            .split('\0')
            .map(|p| p.trim_end_matches('/').to_owned())
            .collect())
    };
    let name = |p: &PathBuf| p.to_string_lossy().trim_end_matches('/').to_owned();
    let index = listed(&["ls-files".as_ref(), "-z".as_ref()])?;
    let on_disk_or_index = paths
        .iter()
        .filter(|p| std::fs::symlink_metadata(root.join(p)).is_ok() || index.contains(&name(p)))
        .collect();
    let in_head = match head {
        Some(head) => {
            let tree = listed(&[
                "ls-tree".as_ref(),
                "-r".as_ref(),
                "-z".as_ref(),
                "--name-only".as_ref(),
                "--full-tree".as_ref(),
                head.as_ref(),
            ])?;
            paths.iter().filter(|p| tree.contains(&name(p))).collect()
        }
        None => Vec::new(),
    };
    Ok((on_disk_or_index, in_head))
}

/// The tree id of everything the working tree of `dir`'s repo holds now (untracked files included, ignored ones
/// not), as a commit of all of it would record. Built in a scratch index in a fresh private dir (never in `.git`,
/// which an agent can write), so the real index is untouched.
pub fn snapshot_tree(dir: &Path) -> Result<String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = repo_root(dir)?;
    let index = PathBuf::from(
        lossy(&git(
            &root,
            &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        )?)
        .trim_end_matches('\n'),
    );
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let scratch_dir = std::env::temp_dir().join(format!(
        "skyelab-snapshot-{}-{n}-{nanos}",
        std::process::id()
    ));
    // Exclusive: fails rather than reuse a dir someone else made.
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut std::fs::DirBuilder::new(), 0o700)
        .create(&scratch_dir)?;
    #[cfg(not(unix))]
    std::fs::create_dir(&scratch_dir)?;
    let scratch = scratch_dir.join("index");
    let tree = (|| {
        // Starting from the real index keeps its stat cache, so unchanged files aren't hashed again.
        match std::fs::copy(&index, &scratch) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        let run = |args: &[&str]| -> Result<Vec<u8>> {
            let mut cmd = command(Path::new("git"), &root);
            cmd.args(args).env("GIT_INDEX_FILE", &scratch);
            crate::finish(&root, args, cmd.output()?)
        };
        run(&["add", "-A"]).and_then(|_| run(&["write-tree"]))
    })();
    let _ = std::fs::remove_dir_all(&scratch_dir);
    Ok(lossy(&tree?).trim().to_owned())
}
