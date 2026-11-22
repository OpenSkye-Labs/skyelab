use std::path::{Path, PathBuf};

use crate::{Error, Result, git, lossy, output, repo_root, status};

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
    let dest = match dest {
        Some(d) => d.to_owned(),
        None => default_dest(&root, branch)?,
    };
    let exists = branch_exists(&root, branch)?;
    if exists && !reuse_branch {
        return Err(Error::BranchExists(branch.to_owned()));
    }
    let dest_arg = dest.as_os_str();
    // `--` keeps a branch, base or path that starts with `-` from being read as an option.
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
        let args = [
            "worktree".as_ref(),
            "add".as_ref(),
            "-b".as_ref(),
            branch.as_ref(),
            "--".as_ref(),
            dest_arg,
            base.as_ref(),
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

/// Removes the worktree at `path`. Refuses with [`Error::Dirty`] if it has any changes, untracked files
/// included, unless `force`. The branch is kept.
pub fn remove_worktree(repo: &Path, path: &Path, force: bool) -> Result<()> {
    if !force && !status(path)?.is_clean() {
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

fn branch_exists(root: &Path, branch: &str) -> Result<bool> {
    // The exit status is the answer, so this skips `git()`, which turns non-zero into an error.
    let out = output(
        Path::new("git"),
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            format!("refs/heads/{branch}").as_str(),
        ],
    )?;
    Ok(out.status.success())
}

fn default_dest(root: &Path, branch: &str) -> Result<PathBuf> {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".into());
    // Hash the full path so two repos with the same folder name don't share a directory.
    let dir = format!("{name}-{:016x}", fnv1a(root.as_os_str().as_encoded_bytes()));
    let data = dirs::data_dir().ok_or(Error::NoDataDir)?;
    Ok(data
        .join("skyelab")
        .join("worktrees")
        .join(dir)
        .join(branch))
}

/// FNV-1a: stable across Rust releases, unlike `DefaultHasher`, so the default paths stay put.
fn fnv1a(bytes: &[u8]) -> u64 {
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
                    _ => {} // locked, prunable: not needed yet
                }
            }
        }
    }
    list.extend(cur);
    list
}
