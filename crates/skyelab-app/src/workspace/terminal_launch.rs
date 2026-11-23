//! What a project terminal launches with beyond skyelab-terminal's inherited allowlist: the folder's account.

use anyhow::Context as _;
use skyelab_core::AccountsConfig;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Env for a terminal in `dir`, on top of the inherited allowlist skyelab-terminal keeps: the bound account's dirs
/// (and its gh as git's https credential helper, when it has one). Every provider home and, unbound, the GitHub dir
/// the account doesn't set points at an empty private dir under `unbound`, so `claude`, `codex` or `gh` typed there
/// asks to log in instead of silently using the default `~/.claude`, `~/.codex` or gh login.
pub(super) fn terminal_env(
    config: &AccountsConfig,
    dir: &Path,
    unbound: &Path,
) -> anyhow::Result<HashMap<String, String>> {
    let text = |p: &Path| {
        p.to_str()
            .map(str::to_owned)
            .with_context(|| format!("{} is not valid UTF-8", p.display()))
    };
    let account = config.resolve(dir);
    let bound = match account {
        Some(_) => config.launch_env(dir)?,
        None => Vec::new(),
    };
    let mut env = HashMap::new();
    let mut homes = vec![("CLAUDE_CONFIG_DIR", "claude"), ("CODEX_HOME", "codex")];
    if account.is_none() {
        homes.push(("GH_CONFIG_DIR", "gh"));
    }
    for (var, sub) in homes {
        if !bound.iter().any(|(k, _)| *k == var) {
            let empty = unbound.join(sub);
            private_dir(&empty)?;
            env.insert(var.to_string(), text(&empty)?);
        }
    }
    for (k, v) in &bound {
        env.insert(k.to_string(), text(v)?);
    }
    if account.is_some_and(|a| a.gh_config_dir.is_some()) {
        let scoped = skyelab_core::env::scoped_extra_headers(dir);
        // ssh stays on: a terminal is the user's own shell, and SSH_AUTH_SOCK passes to it.
        env.extend(skyelab_core::env::git_credential_env(true, &scoped, true));
    }
    Ok(env)
}

/// Where unbound terminals' empty provider homes live: `<data dir>/skyelab/unbound`.
pub(super) fn unbound_root() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("no platform data dir for the unbound terminal homes")?
        .join("skyelab")
        .join("unbound"))
}

fn private_dir(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("making {} private", dir.display()))?;
    }
    Ok(())
}
