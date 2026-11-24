use std::path::{Path, PathBuf};

use crate::{Error, Result, git, git_succeeds, lossy, repo_root, status_with_ignored};

/// One entry of `git worktree list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// Commit checked out; `None` for a bare repo.
    pub head: Option<String>,
    /// Short branch name (`main`, not `refs/heads/main`); `None` when detached or bare.
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
}

/// Adds a worktree of `repo` on a new `branch` started at `base`, and returns its path.
///
/// `dest` defaults to `<data dir>/skyelab/worktrees/<repo name>-<hash>/<branch>`. An existing branch is an
/// error unless `reuse_branch`, in which case it is checked out as is and `base` is ignored.
pub fn create_worktree(
    repo: &Path,
    branch: &str,
    base: &str,
    dest: Option<&Path>,
    reuse_branch: bool,
) -> Result<PathBuf> {
    let root = repo_root(repo)?;
    // Validated before any use: a name like `-D` would reach the `git branch` that `worktree add -b` runs
    // internally as an option, and a `..` component would let the default dest escape its directory.
    let ref_name = format!("refs/heads/{branch}");
    if branch.starts_with('-') || !git_succeeds(&root, &["check-ref-format", &ref_name])? {
        return Err(Error::InvalidBranch(branch.to_owned()));
    }
    let dest = match dest {
        Some(d) => d.to_owned(),
        None => default_dest(&root, branch)?,
    };
    let exists = git_succeeds(&root, &["show-ref", "--verify", "--quiet", &ref_name])?;
    if exists && !reuse_branch {
        return Err(Error::BranchExists(branch.to_owned()));
    }
    let dest_arg = dest.as_os_str();
    // `--` keeps a dest starting with `-` from being read as an option.
    if exists {
        git(
            &root,
            &[
                "worktree".as_ref(),
                "add".as_ref(),
                "--".as_ref(),
                dest_arg,
                branch.as_ref(),
            ],
        )?;
    } else {
        // Resolved to a SHA first, so `base` can't reach the internal `git branch` as an option either.
        let base = git(
            &root,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{base}^{{commit}}"),
            ],
        )?;
        let base = lossy(&base);
        let args = [
            "worktree".as_ref(),
            "add".as_ref(),
            "-b".as_ref(),
            branch.as_ref(),
            "--".as_ref(),
            dest_arg,
            base.trim().as_ref(),
        ];
        git(&root, &args)?;
    }
    Ok(dest)
}

/// Every worktree of `repo`, the main one first.
pub fn list_worktrees(repo: &Path) -> Result<Vec<Worktree>> {
    Ok(parse_list(&git(
        repo,
        &["worktree", "list", "--porcelain", "-z"],
    )?))
}

/// Every folder a hook that Commit, Push or Pull in `dir` would run can be written from: each worktree of the repo,
/// its git common dir (a submodule's or a `--separate-git-dir` one lives outside them) and the hooks dir git would
/// use (`core.hooksPath` may point anywhere). Non-UTF-8 output is an error: a lossy path could name another folder.
pub fn hook_folders(dir: &Path) -> Result<Vec<PathBuf>> {
    let utf8 = |out: Vec<u8>| {
        String::from_utf8(out).map_err(|_| crate::publish::invalid("a repo path isn't UTF-8"))
    };
    let list = git(dir, &["worktree", "list", "--porcelain", "-z"])?;
    utf8(list.clone())?;
    let mut folders: Vec<PathBuf> = parse_list(&list).into_iter().map(|w| w.path).collect();
    // With the repo's own hooks config (no `core.hooksPath=/dev/null`), so `--git-path hooks` names the real dir.
    let args = [
        "rev-parse",
        "--path-format=absolute",
        "--git-common-dir",
        "--git-path",
        "hooks",
    ];
    let mut cmd = crate::hooked_command(Path::new("git"), dir);
    cmd.args(args);
    let out = skyelab_core::process::output(&mut cmd, None)?;
    let out = utf8(crate::finish(dir, &args, out)?)?;
    folders.extend(out.lines().filter(|l| !l.is_empty()).map(PathBuf::from));
    Ok(folders)
}

/// Removes the worktree at `path`. Refuses with [`Error::Dirty`] if it has any changes, untracked and
/// ignored files included (`git worktree remove` deletes ignored files without asking), unless `force`.
/// The branch is kept.
pub fn remove_worktree(repo: &Path, path: &Path, force: bool) -> Result<()> {
    // Absolute, so the dirty check (run in `path`) and the removal (run in `repo`) see the same directory.
    let path = &std::path::absolute(path)?;
    if !force && !status_with_ignored(path)?.is_clean() {
        return Err(Error::Dirty(path.to_owned()));
    }
    let mut args = vec!["worktree".as_ref(), "remove".as_ref()];
    if force {
        args.push("--force".as_ref());
    }
    args.extend(["--".as_ref(), path.as_os_str()]);
    git(repo, &args)?;
    Ok(())
}

pub(crate) fn default_dest(root: &Path, branch: &str) -> Result<PathBuf> {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".into());
    let dir = format!("{name}-{:016x}", fnv1a(root.as_os_str().as_encoded_bytes()));
    let data = dirs::data_dir().ok_or(Error::NoDataDir)?;
    Ok(data
        .join("skyelab")
        .join("worktrees")
        .join(dir)
        .join(branch))
}

/// FNV-1a: stable across Rust releases, unlike `DefaultHasher`, so the default paths stay put.
pub(crate) fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Parses `git worktree list --porcelain -z`: NUL-terminated `key value` fields, an empty field ends a record.
pub(crate) fn parse_list(out: &[u8]) -> Vec<Worktree> {
    let mut list = Vec::new();
    let mut cur: Option<Worktree> = None;
    for field in out.split(|b| *b == 0) {
        let field = lossy(field);
        let (key, value) = field.split_once(' ').unwrap_or((&field, ""));
        match key {
            "worktree" => {
                list.extend(cur.take());
                cur = Some(Worktree {
                    path: value.into(),
                    head: None,
                    branch: None,
                    detached: false,
                    bare: false,
                });
            }
            "" => list.extend(cur.take()),
            _ => {
                let Some(w) = cur.as_mut() else { continue };
                match key {
                    "HEAD" => w.head = Some(value.to_owned()),
                    "branch" => {
                        w.branch = Some(
                            value
                                .strip_prefix("refs/heads/")
                                .unwrap_or(value)
                                .to_owned(),
                        )
                    }
                    "detached" => w.detached = true,
                    "bare" => w.bare = true,
                    _ => {}
                }
            }
        }
    }
    list.extend(cur);
    list
}
