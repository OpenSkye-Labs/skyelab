//! Whether SkyeLab's Commit, Push and Pull may run a repo's own hooks.

use std::path::{Path, PathBuf};

use crate::{AccountsConfig, canonical};

impl AccountsConfig {
    /// Whether Commit, Push and Pull in `dir` may run the repo's own hooks. `folders` are where those hooks could be
    /// written from (`skyelab_git::hook_folders`: every worktree, the git dir, the hooks dir). Each of them, every
    /// bound folder inside one and every bound folder above one must resolve to `dir`'s account: another account's
    /// agent working there could plant a hook that would run with this account's login. Unbound or stale counts as
    /// another account.
    pub fn repo_hooks_allowed(&self, dir: &Path, folders: &[PathBuf]) -> bool {
        let Some(me) = self.resolve(dir) else {
            return false;
        };
        let roots: Vec<PathBuf> = folders.iter().filter_map(|w| canonical(w)).collect();
        if roots.is_empty() || roots.len() != folders.len() {
            return false; // a folder we can't place could be anyone's
        }
        // A stale binding is checked where it points now and where the user confirmed it. Besides folders inside or
        // above a root, a binding whose own `.git` leads into one shares the repo: git's worktree records, which
        // `folders` came from, can be edited to hide a checkout.
        let bound = self
            .bindings
            .iter()
            .flat_map(|b| canonical(&b.path).into_iter().chain(b.resolved.clone()))
            .filter(|p| {
                roots.iter().any(|r| p.starts_with(r) || r.starts_with(p))
                    || git_dirs(p).is_none_or(|dirs| {
                        dirs.iter().any(|d| roots.iter().any(|r| d.starts_with(r)))
                    })
            });
        roots
            .iter()
            .cloned()
            .chain(bound)
            .all(|f| self.resolve(&f).is_some_and(|a| a.id == me.id))
    }
}

/// The git dirs git would use in `dir`, read from disk rather than from git's records: the nearest `.git` at or above
/// it (a dir, or a file's `gitdir:`) or folder git takes as a git dir itself (`HEAD` plus `commondir`), and that
/// dir's `commondir` when it has one. `None` when a `.git` that isn't a dir can't be read as a small regular file
/// (a FIFO would block the read): it could point anywhere.
fn git_dirs(dir: &Path) -> Option<Vec<PathBuf>> {
    use std::io::Read as _;
    let found = dir.ancestors().find_map(|a| {
        let dot = a.join(".git");
        if dot.symlink_metadata().is_ok() {
            Some(dot)
        } else {
            (a.join("HEAD").is_file() && a.join("commondir").is_file()).then(|| a.to_owned())
        }
    });
    let Some(dot) = found else {
        return Some(Vec::new());
    };
    let gitdir = if dot.is_dir() {
        dot
    } else {
        if !std::fs::metadata(&dot).ok()?.is_file() {
            return None;
        }
        let mut text = String::new();
        std::fs::File::open(&dot)
            .ok()?
            .take(4096)
            .read_to_string(&mut text)
            .ok()?;
        let target = text.lines().next()?.strip_prefix("gitdir:")?.trim();
        dot.parent()?.join(target)
    };
    let common = std::fs::read_to_string(gitdir.join("commondir"))
        .ok()
        .map(|c| gitdir.join(c.trim()));
    Some(
        std::iter::once(gitdir)
            .chain(common)
            .filter_map(|d| canonical(&d))
            .collect(),
    )
}
