//! Which `gh` SkyeLab runs, and the terminal that logs an account in to GitHub.

use anyhow::Context as _;
use gpui::App;
use skyelab_terminal::SpawnOptions;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The `gh` that pushes, opens PRs and logs accounts in: `gh`, or in debug builds `SKYELAB_GH_BIN` (a fake).
pub struct GhBin(pub PathBuf);

impl gpui::Global for GhBin {}

pub(crate) fn gh_bin(cx: &App) -> PathBuf {
    cx.try_global::<GhBin>()
        .map_or_else(|| "gh".into(), |g| g.0.clone())
}

/// The `codex` that Codex threads and Codex logins run: `codex`, or in debug builds `SKYELAB_CODEX_BIN` (a fake).
pub struct CodexBin(pub PathBuf);

impl gpui::Global for CodexBin {}

pub(crate) fn codex_bin(cx: &App) -> PathBuf {
    cx.try_global::<CodexBin>()
        .map_or_else(|| "codex".into(), |g| g.0.clone())
}

/// A terminal running `codex login` with `CODEX_HOME=<dir>` only, so the login lands in that account's dir.
/// Codex refuses a `CODEX_HOME` that doesn't exist, so the dir is made (private) first.
pub(crate) fn codex_login_options(codex: &Path, dir: &Path) -> anyhow::Result<SpawnOptions> {
    let dir_text = dir
        .to_str()
        .with_context(|| format!("{} is not valid UTF-8", dir.display()))?;
    let codex = codex
        .to_str()
        .with_context(|| format!("{} is not valid UTF-8", codex.display()))?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("making {} private", dir.display()))?;
    }
    Ok(SpawnOptions {
        cwd: dirs::home_dir().unwrap_or_else(|| dir.to_owned()),
        command: Some((codex.into(), vec!["login".into()])),
        env: HashMap::from([("CODEX_HOME".to_string(), dir_text.to_string())]),
        cols: 80,
        lines: 24,
    })
}

/// A terminal running `claude auth login` with `account`'s launch env (its `CLAUDE_CONFIG_DIR`, and its
/// `GH_CONFIG_DIR` when set), so the login lands in that account's config dir and nowhere else. The dir is made
/// private first, like a Codex home.
pub(crate) fn claude_login_options(
    claude: &Path,
    account: &skyelab_core::Account,
) -> anyhow::Result<SpawnOptions> {
    let claude = claude
        .to_str()
        .with_context(|| format!("{} is not valid UTF-8", claude.display()))?;
    let mut env = HashMap::new();
    for (var, path) in account.launch_env()? {
        let text = path
            .to_str()
            .with_context(|| format!("{} is not valid UTF-8", path.display()))?;
        env.insert(var.to_string(), text.to_string());
    }
    let dir = &account.config_dir;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("making {} private", dir.display()))?;
    }
    Ok(SpawnOptions {
        cwd: dirs::home_dir().unwrap_or_else(|| dir.to_owned()),
        command: Some((claude.into(), vec!["auth".into(), "login".into()])),
        env,
        cols: 80,
        lines: 24,
    })
}

/// A terminal running `gh auth login` under `dir` only. `--insecure-storage` keeps the token in
/// `<dir>/hosts.yml`: gh's keyring entry for the active user is shared by every config dir.
pub(crate) fn gh_login_options(gh: &Path, dir: &Path) -> anyhow::Result<SpawnOptions> {
    let dir_text = dir
        .to_str()
        .with_context(|| format!("{} is not valid UTF-8", dir.display()))?;
    let gh = gh
        .to_str()
        .with_context(|| format!("{} is not valid UTF-8", gh.display()))?;
    Ok(SpawnOptions {
        cwd: dirs::home_dir().unwrap_or_else(|| dir.to_owned()),
        command: Some((
            gh.into(),
            vec!["auth".into(), "login".into(), "--insecure-storage".into()],
        )),
        env: HashMap::from([("GH_CONFIG_DIR".to_string(), dir_text.to_string())]),
        cols: 80,
        lines: 24,
    })
}
