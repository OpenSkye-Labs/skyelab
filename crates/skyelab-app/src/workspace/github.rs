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

/// The `agy` that Antigravity threads and logins run: `agy`, or in debug builds `SKYELAB_ANTIGRAVITY_BIN` (a fake).
pub struct AntigravityBin(pub PathBuf);

impl gpui::Global for AntigravityBin {}

pub(crate) fn antigravity_bin(cx: &App) -> PathBuf {
    cx.try_global::<AntigravityBin>()
        .map_or_else(skyelab_agent::antigravity::default_bin, |g| g.0.clone())
}

/// What logs an account in to its agent CLI.
pub(crate) fn login_args(provider: skyelab_core::Provider) -> &'static [&'static str] {
    match provider {
        skyelab_core::Provider::Claude => &["auth", "login"],
        skyelab_core::Provider::Codex => &["login"],
        skyelab_core::Provider::Antigravity => &["acp", "--login"],
    }
}

/// What signs an account in to one of its MCP servers: `claude mcp login` and `codex mcp login` alike. `--` keeps a
/// server named like a flag from being read as one.
pub(crate) fn mcp_login_args(server: &str) -> [&str; 4] {
    ["mcp", "login", "--", server]
}

/// A terminal running `<bin> <args>` (`claude auth login`, `codex login`, `agy acp --login`) with `account`'s launch
/// env only, so the login lands in that account's config dir. Codex and Antigravity need their config dir created
/// (private) first; Antigravity also adds keyring-isolation env and `--gemini-dir` / `--app-data-dir`.
pub(crate) fn agent_login_options(
    bin: &Path,
    args: &[&str],
    account: &skyelab_core::Account,
) -> anyhow::Result<SpawnOptions> {
    let utf8 = |p: &Path| {
        p.to_str()
            .map(str::to_owned)
            .with_context(|| format!("{} is not valid UTF-8", p.display()))
    };
    let mut env = HashMap::new();
    for (var, path) in account.launch_env()? {
        env.insert(var.to_string(), utf8(&path)?);
    }
    let dir = &account.config_dir;
    let command_args = if account.provider == skyelab_core::Provider::Antigravity {
        skyelab_agent::antigravity::ensure_profile_dirs(dir)?;
        for (k, v) in skyelab_agent::antigravity::isolated_env() {
            env.insert(k.to_string(), v.to_string());
        }
        if args == login_args(skyelab_core::Provider::Antigravity) {
            skyelab_agent::antigravity::login_args(dir)?
        } else {
            args.iter().map(|a| (*a).to_string()).collect()
        }
    } else {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                .with_context(|| format!("making {} private", dir.display()))?;
        }
        args.iter().map(|a| (*a).to_string()).collect()
    };
    Ok(SpawnOptions {
        cwd: dirs::home_dir().unwrap_or_else(|| dir.to_owned()),
        command: Some((utf8(bin)?, command_args)),
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
