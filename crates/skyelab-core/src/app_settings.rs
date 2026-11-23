//! App preferences that aren't accounts: `settings.toml`, next to `accounts.toml`.

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppSettings {
    /// Serve this machine to the user's own tailnet devices. Off unless the user turns it on.
    pub share_on_tailnet: bool,
    /// Remote environments (MagicDNS names or tailnet IPs) the user picked, most recent first.
    pub recent_environments: Vec<String>,
    /// MagicDNS name (lowercase) → the Tailscale node StableID it answered from when it was trusted. A name that
    /// later answers from another node (a reused device name in a shared tailnet) is refused until trusted again.
    pub environment_nodes: BTreeMap<String, String>,
    /// Devices allowed to use this machine while it is shared: a client install's key on one tailnet node.
    pub approved_devices: Vec<ApprovedDevice>,
    /// The main window's last frame and right panel, restored at the next launch.
    pub window: Option<WindowLayout>,
    /// "Open in editor": the command to run, split on whitespace (no shell), the folder appended. Unset: `$VISUAL`,
    /// `$EDITOR`, then `code` or `zed` from PATH.
    pub editor: Option<String>,
}

/// Where the main window was and how its right panel was set, in logical points.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowLayout {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub right_panel_open: bool,
    pub right_panel_width: i32,
}

/// One device the user allowed in the Remote tab.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ApprovedDevice {
    /// The node's Tailscale StableID.
    pub node_id: String,
    /// SHA-256 of the client's device key, hex. The key itself is never stored here.
    pub key_hash: String,
    /// The MagicDNS name and login it had when allowed, for the list.
    pub name: String,
    pub user: String,
}

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

    /// The node `host` was trusted as, if any.
    pub fn environment_node(&self, host: &str) -> Option<&str> {
        self.environment_nodes
            .get(&host.trim().to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn trust_environment_node(&mut self, host: &str, node_id: &str) {
        self.environment_nodes
            .insert(host.trim().to_ascii_lowercase(), node_id.to_owned());
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

    /// Load, change, save, under one process-wide lock, so two writers (recents, sharing, node pins) can't each
    /// save what they read before the other's write and drop it. A file that doesn't parse is left alone.
    pub fn update<T>(path: &Path, change: impl FnOnce(&mut Self) -> T) -> Result<(Self, T)> {
        static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _held = WRITE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut settings = Self::load(path)?;
        let out = change(&mut settings);
        settings.save(path)?;
        Ok((settings, out))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).context("serializing settings")?;
        crate::write_atomic(path, &text)
    }
}
