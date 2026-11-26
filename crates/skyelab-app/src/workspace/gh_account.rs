//! The GitHub login that acts for a folder: its binding's account, checked against the account the caller showed.

use super::worktree::resolves_to;
use anyhow::bail;
use skyelab_core::AccountsConfig;
use std::path::{Path, PathBuf};

/// The bound account's gh config dir for `dir`, read from `config` (accounts.toml as saved now): the folder must
/// still be bound to `account`, and that account signed in to GitHub.
pub(super) fn account_gh_dir(
    config: &AccountsConfig,
    dir: &Path,
    account: &str,
) -> anyhow::Result<PathBuf> {
    resolves_to(config, dir, account)?;
    let gh_dir = config.github_config_dir(dir)?;
    if !skyelab_git::gh_logged_in(&gh_dir) {
        bail!("{account} isn't signed in to GitHub. Sign in under Settings, Source control.");
    }
    Ok(gh_dir)
}
