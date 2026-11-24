//! App preferences that aren't accounts: `settings.toml`, next to `accounts.toml`.

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[cfg(test)]
#[path = "sidebar_views_test.rs"]
mod sidebar_views_test;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppSettings {
    /// Serve this machine to the user's own tailnet devices. Off unless the user turns it on.
    pub share_on_tailnet: bool,
    /// Remote environments (MagicDNS names or tailnet IPs) the user picked, most recent first.
    pub recent_environments: Vec<String>,
    /// Machines (MagicDNS names) whose threads also show in this machine's sidebar, in the order they were linked.
    pub linked_environments: Vec<String>,
    /// MagicDNS name (lowercase) → the Tailscale node StableID it answered from when it was trusted. A name that
    /// later answers from another node (a reused device name in a shared tailnet) is refused until trusted again.
    pub environment_nodes: BTreeMap<String, String>,
    /// Devices allowed to use this machine while it is shared: a client install's key on one tailnet node.
    pub approved_devices: Vec<ApprovedDevice>,
    /// Older files' single window slot; read when `windows` is empty, cleared on the next save.
    pub window: Option<WindowLayout>,
    /// Every window open at the last save (frame, right panel, page), in the order they opened; all reopen at launch.
    pub windows: Vec<WindowLayout>,
    /// "Open in editor": the command to run, split on whitespace (no shell), the folder appended. Unset: `$VISUAL`,
    /// `$EDITOR`, then `code` or `zed` from PATH.
    pub editor: Option<String>,
    /// What new threads start with (Settings → Chat, Diffs & pull requests, Worktrees).
    pub threads: ThreadSettings,
    /// The Changes tab's view toggles.
    pub diff: DiffSettings,
    /// Settings → General: the update check.
    pub updates: crate::update::UpdateSettings,
    /// Settings → Appearance: theme, accent, density and text sizes.
    pub appearance: AppearanceSettings,
    /// Settings → Scheduled tasks: missed runs and how many run at once.
    pub scheduled: ScheduledSettings,
    /// Settings → General: the terminal.
    pub terminal: TerminalSettings,
    /// The sidebar's saved views (T3's saved views), in the order they were saved.
    pub sidebar_views: Vec<SidebarView>,
}

/// A named sidebar filter: the scope's projects, by folder (a project's id changes if it is added again), and the
/// filter text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SidebarView {
    pub name: String,
    /// Empty: all projects.
    pub projects: Vec<PathBuf>,
    pub search: String,
}

/// Settings → General's terminal switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalSettings {
    /// Ask before closing a terminal pane or tab whose shell is running a command (T3's terminal close confirmation).
    pub confirm_close: bool,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            confirm_close: true,
        }
    }
}

/// Light, dark, or whatever the system shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    #[default]
    System,
    Light,
    Dark,
}

/// Settings → Appearance. Names are the app's theme and accent keys; an unknown one falls back to the default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppearanceSettings {
    pub mode: ColorMode,
    /// The theme family (`skyelab`, `skye`, …).
    pub theme: String,
    /// An accent preset key; unset is the theme's own.
    pub accent: Option<String>,
    /// Tighter rows and cards.
    pub compact: bool,
    /// Interface text size in points.
    pub ui_font_size: u8,
    /// Code and terminal text size in points.
    pub code_font_size: u8,
}

impl AppearanceSettings {
    pub const UI_SIZES: std::ops::RangeInclusive<u8> = 12..=18;
    pub const CODE_SIZES: std::ops::RangeInclusive<u8> = 11..=18;

    /// The sizes clamped to what the UI offers, so a hand-edited file can't make text vanish.
    pub fn clamped(mut self) -> Self {
        self.ui_font_size = self
            .ui_font_size
            .clamp(*Self::UI_SIZES.start(), *Self::UI_SIZES.end());
        self.code_font_size = self
            .code_font_size
            .clamp(*Self::CODE_SIZES.start(), *Self::CODE_SIZES.end());
        self
    }
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            mode: ColorMode::System,
            theme: "skyelab".into(),
            accent: None,
            compact: false,
            ui_font_size: 14,
            code_font_size: 13,
        }
    }
}

/// How the scheduler treats runs. The tasks themselves live in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScheduledSettings {
    /// A run missed while SkyeLab wasn't running is skipped instead of run once at the next start.
    pub skip_missed: bool,
    /// Scheduled runs starting or working at once, app-wide; a due task waits for a free one.
    pub max_concurrent: u32,
}

impl Default for ScheduledSettings {
    fn default() -> Self {
        Self {
            skip_missed: false,
            max_concurrent: 2,
        }
    }
}

/// How the Changes tab draws a diff (T3's diff layout, word wrap and whitespace settings).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiffSettings {
    /// Old and new side by side; off is one unified column.
    pub split: bool,
    /// Long lines wrap instead of being clipped.
    pub wrap: bool,
    /// Whitespace-only changes are hidden (`git diff -w`).
    pub ignore_whitespace: bool,
}

/// The defaults a new thread starts with. A thread's own pick (and a project's remembered model) still wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThreadSettings {
    /// After a thread's first reply, its account's agent writes a short title for it.
    pub generate_titles: bool,
    /// The Changes tab offers a generated commit message and PR title and body (written by the thread's own account's
    /// agent, only when asked).
    pub generate_git_text: bool,
    /// The permission mode (the agent layer's value: `default`, `plan`, `acceptEdits`); unset is Ask.
    pub permission_mode: Option<String>,
    pub claude_model: Option<String>,
    pub claude_effort: Option<String>,
    pub codex_model: Option<String>,
    pub codex_effort: Option<String>,
    /// New threads start with "New worktree" on.
    pub new_worktree: bool,
    /// Minutes between background `git fetch`es of the shown thread's folder; 0 is off.
    pub fetch_minutes: u32,
    /// Project folder → its own new-thread defaults over these (Settings' "Applying settings for <project>").
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub projects: BTreeMap<PathBuf, crate::thread_defaults::ThreadDefaults>,
}

impl ThreadSettings {
    /// The default model and effort for `provider`'s new threads.
    pub fn model_for(&self, provider: crate::Provider) -> (Option<String>, Option<String>) {
        match provider {
            crate::Provider::Claude => (self.claude_model.clone(), self.claude_effort.clone()),
            crate::Provider::Codex => (self.codex_model.clone(), self.codex_effort.clone()),
        }
    }
}

impl Default for ThreadSettings {
    fn default() -> Self {
        Self {
            generate_titles: true,
            generate_git_text: true,
            permission_mode: None,
            claude_model: None,
            claude_effort: None,
            codex_model: None,
            codex_effort: None,
            new_worktree: false,
            fetch_minutes: 5,
            projects: BTreeMap::new(),
        }
    }
}

/// Where the main window was, how its right panel was set (logical points), and which page it showed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowLayout {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub right_panel_open: bool,
    pub right_panel_width: i32,
    /// The page the center showed: `thread:<id>`, `settings:<section>` or `welcome` (empty in older files).
    pub view: String,
    /// The window's terminal tabs, in tab order; they reopen with it (new shells, same layout and folders).
    pub terminals: Vec<crate::terminal_layout::SavedTerminalTab>,
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

    /// The windows to open at launch: the list, else an older file's single slot.
    pub fn saved_windows(&self) -> Vec<WindowLayout> {
        if self.windows.is_empty() {
            self.window.iter().cloned().collect()
        } else {
            self.windows.clone()
        }
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

    /// Links `host` (its threads show in this machine's list), or unlinks it when it already is. Returns whether it
    /// is linked now. Names compare case-insensitively, as DNS does; a blank name changes nothing.
    pub fn toggle_linked_environment(&mut self, host: &str) -> bool {
        let host = host.trim();
        if host.is_empty() {
            return false;
        }
        let before = self.linked_environments.len();
        self.linked_environments
            .retain(|h| !h.eq_ignore_ascii_case(host));
        let linked = self.linked_environments.len() == before;
        if linked {
            self.linked_environments.push(host.to_owned());
        }
        linked
    }

    pub fn is_linked_environment(&self, host: &str) -> bool {
        let host = host.trim();
        self.linked_environments
            .iter()
            .any(|h| h.eq_ignore_ascii_case(host))
    }

    /// Drops `host` from the recents. Its trusted node stays, so connecting again still checks the same machine.
    pub fn forget_environment(&mut self, host: &str) {
        let host = host.trim();
        self.recent_environments
            .retain(|h| !h.eq_ignore_ascii_case(host));
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
