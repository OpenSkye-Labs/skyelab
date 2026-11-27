//! The env and hook policy Commit, Push and Pull run the repo's git hooks with.

use anyhow::{Context as _, Result};
use skyelab_core::AccountsConfig;
use std::ffi::OsString;
use std::path::Path;

/// `note`, plus why the repo's hooks didn't run when they were off (`hooks` as [`hook_env`] returns it).
pub(super) fn hooks_note(note: String, hooks: Option<bool>) -> String {
    match hooks {
        Some(true) => note,
        Some(false) => format!("{note}. Hooks skipped: this repo is shared with another account"),
        None => format!("{note}. Hooks skipped: couldn't confirm this repo is single-account"),
    }
}

pub(super) type HookEnv = Vec<(OsString, OsString)>;

/// The env the repo's hooks run with on Commit, Push and Pull: the bound account's launch env plus the git credential
/// binding its agents get (only its gh answers https, ssh only if the account allows it), and no prompts. The bool
/// says whether hooks may run at all: not while another account (or no account) shares the repo's git dir, since its
/// agents could have written them; `None` when the repo's folders couldn't be listed, so nobody knows.
pub(super) fn hook_env(config: &AccountsConfig, dir: &Path) -> Result<(HookEnv, Option<bool>)> {
    let account = config
        .resolve(dir)
        .with_context(|| format!("{} isn't bound to an account", dir.display()))?;
    let mut env: Vec<(OsString, OsString)> = account
        .launch_env()?
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
    let git = skyelab_core::env::account_git_env(account, dir);
    env.extend(git.into_iter().map(|(k, v)| (k.into(), v.into())));
    // ssh commit signing needs the agent; only for accounts that let their agents use it too, since the repo's
    // hooks (agent-writable) see the same env.
    if account.ssh_agent
        && let Some(sock) = std::env::var_os("SSH_AUTH_SOCK")
    {
        env.push(("SSH_AUTH_SOCK".into(), sock));
    }
    // A repo whose folders can't be listed keeps hooks off: the safe side.
    let hooks = skyelab_git::hook_folders(dir)
        .ok()
        .filter(|folders| !folders.is_empty())
        .map(|folders| config.repo_hooks_allowed(dir, &folders));
    Ok((env, hooks))
}
