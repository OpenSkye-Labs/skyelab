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
        // A stale binding is checked where it points now and where the user confirmed it.
        let bound = self
            .bindings
            .iter()
            .flat_map(|b| canonical(&b.path).into_iter().chain(b.resolved.clone()))
            .filter(|p| roots.iter().any(|r| p.starts_with(r) || r.starts_with(p)));
        roots
            .iter()
            .cloned()
            .chain(bound)
            .all(|f| self.resolve(&f).is_some_and(|a| a.id == me.id))
    }
}
