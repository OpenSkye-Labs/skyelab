//! Where accounts.toml lives, and reading it as saved right now: launches, terminals and git actions check the
//! binding against the file, not the window's copy of it.

use std::path::PathBuf;

/// The accounts.toml a launch re-reads right before it spawns.
pub struct AccountsFile(pub PathBuf);

impl gpui::Global for AccountsFile {}

/// The accounts as `file` says right now (`fallback` without one). An unreadable file binds nothing.
pub fn accounts_now(
    file: Option<&std::path::Path>,
    fallback: &skyelab_core::AccountsConfig,
) -> skyelab_core::AccountsConfig {
    file.map_or_else(
        || fallback.clone(),
        |p| skyelab_core::AccountsConfig::load(p).unwrap_or_default(),
    )
}

/// The label of the account `folder` is bound to. `None` means unbound: the UI says so, never guesses.
pub(super) fn account_label(
    config: &skyelab_core::AccountsConfig,
    folder: &std::path::Path,
) -> Option<gpui::SharedString> {
    config.resolve(folder).map(|a| a.label.clone().into())
}
