//! App preferences that aren't accounts: `settings.toml`, next to `accounts.toml`.

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppSettings {
    /// Serve this machine to the user's own tailnet devices. Off unless the user turns it on.
    pub share_on_tailnet: bool,
}

impl AppSettings {
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("skyelab").join("settings.toml"))
    }

    /// A missing file is the defaults.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).context("serializing settings")?;
        crate::write_atomic(path, &text)
    }
}
