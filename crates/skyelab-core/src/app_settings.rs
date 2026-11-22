//! App preferences that aren't accounts: `settings.toml`, next to `accounts.toml`.

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppSettings {
    /// Serve this machine to the user's own tailnet devices. Off unless the user turns it on.
    pub share_on_tailnet: bool,
    /// Remote environments (MagicDNS names) the user picked, most recent first.
    pub recent_environments: Vec<String>,
}

/// How many recent environments the picker remembers.
pub const MAX_RECENT_ENVIRONMENTS: usize = 8;

impl AppSettings {
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("skyelab").join("settings.toml"))
    }

    /// Puts `host` first in the recents, once (names compare case-insensitively, as DNS does).
    pub fn remember_environment(&mut self, host: &str) {
        let host = host.trim();
        if host.is_empty() {
            return;
        }
        self.recent_environments
            .retain(|h| !h.eq_ignore_ascii_case(host));
        self.recent_environments.insert(0, host.to_owned());
        self.recent_environments.truncate(MAX_RECENT_ENVIRONMENTS);
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
