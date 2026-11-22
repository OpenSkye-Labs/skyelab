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
