//! UI-free domain logic. Nothing in this crate may depend on GPUI.

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub mod add_project;
mod app_settings;
pub mod context;
pub mod debug;
pub mod delegation;
pub mod dictation;
mod editor;
pub mod schedule;
pub use editor::editor_argv;
mod pr_url;
pub use pr_url::github_pr_url;
pub mod env;
pub mod files;
pub mod instance;
pub mod keymap;
pub mod onboarding;
pub mod page_hosts;
mod write_atomic;
use write_atomic::write_atomic;
pub mod process;
pub mod project_import;
pub mod project_rules;
pub mod prompt;
pub mod scratch;
pub mod snooze;
pub use app_settings::{
    AppSettings, AppearanceSettings, ApprovedDevice, ColorMode, DiffSettings,
    MAX_RECENT_ENVIRONMENTS, MotionMode, ScheduledSettings, SendShortcut, SidebarView,
    TerminalSettings, ThreadSettings, VisualReplySettings, WindowLayout,
};
mod repo_hooks;
pub mod terminal_context;
pub mod terminal_layout;
pub mod thread_defaults;
pub mod update;
pub mod update_install;

/// Which agent CLI an account logs in to. Config files from before Codex have no `provider`: Claude.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Claude,
    Codex,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Claude, Provider::Codex];

    pub fn label(self) -> &'static str {
        match self {
            Provider::Claude => "Claude",
            Provider::Codex => "Codex",
        }
    }

    /// The env var that points the CLI at an account's config dir.
    pub fn home_var(self) -> &'static str {
        match self {
            Provider::Claude => "CLAUDE_CONFIG_DIR",
            Provider::Codex => "CODEX_HOME",
        }
    }

    fn is_claude(&self) -> bool {
        *self == Provider::Claude
    }
}

/// One provider login. An account *is* its config dir (`CLAUDE_CONFIG_DIR` or `CODEX_HOME`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Provider::is_claude")]
    pub provider: Provider,
    pub config_dir: PathBuf,
    /// This account's `gh` config dir (`GH_CONFIG_DIR`): its GitHub login. `None`: SkyeLab's own push and PR
    /// refuse, and agents get gh's default config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gh_config_dir: Option<PathBuf>,
    /// Let this account's agents use the user's ssh-agent (`SSH_AUTH_SOCK`) and git's ssh transport. Off by
    /// default: an ssh key isn't bound to any account, so an agent's ssh push could land as anyone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ssh_agent: bool,
}

impl Account {
    /// The same login: every field but the display label. A launch check compares this, so a renamed account
    /// still launches and a moved config dir, gh login, provider or ssh opt-in does not.
    pub fn same_login(&self, other: &Account) -> bool {
        let Account {
            id,
            label: _,
            provider,
            config_dir,
            gh_config_dir,
            ssh_agent,
        } = self;
        (id, provider, config_dir, gh_config_dir, ssh_agent)
            == (
                &other.id,
                &other.provider,
                &other.config_dir,
                &other.gh_config_dir,
                &other.ssh_agent,
            )
    }

    /// Env to launch this account's agent CLI with: its provider's home var, and its GitHub dir.
    pub fn launch_env(&self) -> Result<Vec<(&'static str, PathBuf)>> {
        check_account(self)?; // the fields are public, so this account may never have been validated
        let gh = self.gh_config_dir.clone().map(|d| ("GH_CONFIG_DIR", d));
        Ok([(self.provider.home_var(), self.config_dir.clone())]
            .into_iter()
            .chain(gh)
            .collect())
    }
}

/// Every folder under `path` runs agents as `account`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub path: PathBuf,
    pub account: String,
    /// The real folder `path` was when the user confirmed it. Private so callers can't write
    /// one by hand; load and save carry it as is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolved: Option<PathBuf>,
}

impl Binding {
    /// Snapshots the folder `path` resolves to now (`None`, so stale, if it can't be resolved).
    /// `add_binding` takes the snapshot again, so the bind moment is what counts.
    pub fn new(path: PathBuf, account: String) -> Self {
        let resolved = binding_root(&path).ok();
        Self {
            path,
            account,
            resolved,
        }
    }

    /// The real folder last confirmed (bind or re-confirm). `None` when never confirmed: an
    /// older config, or a path that couldn't be resolved when the `Binding` was built.
    pub fn resolved(&self) -> Option<&Path> {
        self.resolved.as_deref()
    }

    /// The real folder `path` resolves to right now. `None` when it can't be resolved (a broken link).
    pub fn current_root(&self) -> Option<PathBuf> {
        binding_root(&self.path).ok()
    }

    /// True when `path` no longer resolves to the confirmed folder (swapped for a symlink,
    /// link retargeted, broken), or was never confirmed. A stale binding never picks an account.
    pub fn is_stale(&self) -> bool {
        self.resolved.is_none() || self.current_root() != self.resolved
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountsConfig {
    #[serde(default, rename = "account")]
    pub accounts: Vec<Account>,
    #[serde(default, rename = "binding")]
    pub bindings: Vec<Binding>,
}

impl AccountsConfig {
    pub fn default_path() -> Option<PathBuf> {
        instance::config_file("accounts.toml")
    }

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let cfg: Self =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        let mut ids = std::collections::HashSet::new();
        for a in &self.accounts {
            check_account(a)?;
            if !ids.insert(a.id.as_str()) {
                bail!("account '{}' is listed twice", a.id);
            }
        }
        let mut paths = std::collections::HashMap::new();
        for b in &self.bindings {
            check_bound_path(&b.path)?;
            if !self.accounts.iter().any(|a| a.id == b.account) {
                bail!(
                    "binding {} points at unknown account '{}'",
                    b.path.display(),
                    b.account
                );
            }
            // A path that can't be resolved now (a broken link) loads as stale, so the user can fix it.
            let Some(real) = canonical(&b.path) else {
                continue;
            };
            if let Some(first) = paths.insert(real, &b.path) {
                bail!(
                    "{} and {} are the same folder, bound twice",
                    first.display(),
                    b.path.display()
                );
            }
        }
        Ok(())
    }

    /// Validate, then write atomically (temp file in the same dir, then rename), creating the parent dir.
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let text = toml::to_string_pretty(self).context("serializing accounts config")?;
        write_atomic(path, &text)
    }

    pub fn add_account(&mut self, account: Account) -> Result<()> {
        check_account(&account)?;
        if self.accounts.iter().any(|a| a.id == account.id) {
            bail!("account '{}' already exists", account.id);
        }
        self.check_own_dir(&account)?;
        self.accounts.push(account);
        Ok(())
    }

    /// An account is its config dir: no two accounts share one.
    fn check_own_dir(&self, account: &Account) -> Result<()> {
        let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.into());
        let mine = real(&account.config_dir);
        match self
            .accounts
            .iter()
            .find(|a| a.id != account.id && real(&a.config_dir) == mine)
        {
            Some(o) => bail!("account '{}' already uses {}", o.id, o.config_dir.display()),
            None => Ok(()),
        }
    }

    /// Replace the label and config dirs of the account with the same id. Its provider stays: its
    /// threads hold that provider's session ids, which the other CLI can't resume.
    pub fn update_account(&mut self, account: Account) -> Result<()> {
        check_account(&account)?;
        self.check_own_dir(&account)?;
        let slot = self
            .accounts
            .iter_mut()
            .find(|a| a.id == account.id)
            .with_context(|| format!("unknown account '{}'", account.id))?;
        if slot.provider != account.provider {
            bail!(
                "account '{}' is a {} account; its provider can't change. Add a new account instead",
                account.id,
                slot.provider.label()
            );
        }
        *slot = account;
        Ok(())
    }

    /// Refuses while any binding still uses the account. Never cascades.
    pub fn remove_account(&mut self, id: &str) -> Result<()> {
        let bound: Vec<_> = self
            .bindings
            .iter()
            .filter(|b| b.account == id)
            .map(|b| b.path.display().to_string())
            .collect();
        if !bound.is_empty() {
            bail!(
                "account '{id}' is still bound to {}; remove those bindings first",
                bound.join(", ")
            );
        }
        let before = self.accounts.len();
        self.accounts.retain(|a| a.id != id);
        if self.accounts.len() == before {
            bail!("unknown account '{id}'");
        }
        Ok(())
    }

    /// Binding is the user confirming the folder, so the snapshot is taken here whatever the caller passed.
    pub fn add_binding(&mut self, mut binding: Binding) -> Result<()> {
        if !self.accounts.iter().any(|a| a.id == binding.account) {
            bail!("unknown account '{}'", binding.account);
        }
        let new = binding_root(&binding.path)?;
        self.check_unclaimed(&new, None)?;
        binding.resolved = Some(new);
        self.bindings.push(binding);
        Ok(())
    }

    /// The user saw that `path` now resolves to `now` and says that's right. Refused if it
    /// resolves elsewhere by now, is the disk root, contains the folder confirmed before, or
    /// is a folder another binding owns or owned.
    pub fn reconfirm_binding(&mut self, path: &Path, now: &Path) -> Result<()> {
        let i = self
            .bindings
            .iter()
            .position(|b| b.path == path)
            .with_context(|| format!("{} is not bound", path.display()))?;
        let root = binding_root(path)?;
        if root != now {
            bail!(
                "{} now resolves to {}, not {}; check it again",
                path.display(),
                root.display(),
                now.display()
            );
        }
        let old = self.bindings[i].resolved.as_deref();
        if root.parent().is_none() || old.is_some_and(|old| old != root && old.starts_with(&root)) {
            bail!(
                "{} now resolves to {}, which would cover far more than the folder you confirmed; \
                 remove the binding and bind the folder again if that's really what you want",
                path.display(),
                root.display()
            );
        }
        self.check_unclaimed(&root, Some(i))?;
        self.bindings[i].resolved = Some(root);
        Ok(())
    }

    /// `root` isn't where another binding points now, or pointed when confirmed (a stale one
    /// still blocks there, so a new binding on it could never resolve).
    fn check_unclaimed(&self, root: &Path, except: Option<usize>) -> Result<()> {
        let other = self.bindings.iter().enumerate().find(|(j, b)| {
            Some(*j) != except
                && (b.current_root().as_deref() == Some(root) || b.resolved() == Some(root))
        });
        if let Some((_, b)) = other {
            bail!(
                "{} is already bound by {}; re-confirm or remove that binding first",
                root.display(),
                b.path.display()
            );
        }
        Ok(())
    }

    pub fn remove_binding(&mut self, path: &Path) -> Result<()> {
        let before = self.bindings.len();
        self.bindings.retain(|b| b.path != path);
        if self.bindings.len() == before {
            bail!("{} is not bound", path.display());
        }
        Ok(())
    }

    /// The account bound to `dir`: deepest binding wins. `None` means unbound, and the
    /// caller must ask the user. Never fall back to a default account, that is the bug we exist to prevent.
    pub fn resolve(&self, dir: &Path) -> Option<&Account> {
        let b = self.binding_for(dir)?;
        self.accounts.iter().find(|a| a.id == b.account)
    }

    /// The binding [`AccountsConfig::resolve`] goes by for `dir`: the folder that decides its account.
    pub fn binding_for(&self, dir: &Path) -> Option<&Binding> {
        if !dir.is_absolute() {
            return None;
        }
        let dir = canonical(dir)?;
        let mut roots = Vec::new();
        for b in &self.bindings {
            // A malformed binding (an unvalidated config) means nothing resolves, rather than a shallower one winning.
            check_bound_path(&b.path).ok()?;
            // Resolved once, so the match and the stale check can't see two different filesystems.
            let now = canonical(&b.path);
            let stale = b.resolved.is_none() || now != b.resolved;
            // A stale binding still matches where it points now and where it pointed when
            // confirmed, so a shallower binding can't silently take those folders over.
            let snapshot = b.resolved.clone().filter(|_| stale);
            roots.extend(now.into_iter().chain(snapshot).map(|r| (r, b, stale)));
        }
        let matching: Vec<_> = roots
            .iter()
            .filter(|(root, ..)| dir.starts_with(root)) // component-wise, so /work/acme does not match /work/acme-old
            .collect();
        let depth = matching
            .iter()
            .map(|(root, ..)| root.components().count())
            .max()?;
        let mut deepest = matching
            .iter()
            .filter(|(root, ..)| root.components().count() == depth);
        let (_, b, stale) = deepest.next()?;
        if *stale || deepest.next().is_some() {
            return None; // not the folder the user confirmed, or two bindings now name the same folder: ask
        }
        Some(b)
    }

    /// Env to launch the bound account's agent with for `dir`. Errors when the folder is unbound.
    pub fn launch_env(&self, dir: &Path) -> Result<Vec<(&'static str, PathBuf)>> {
        self.resolve(dir)
            .with_context(|| format!("{} is not bound to an account", dir.display()))?
            .launch_env()
    }

    /// The `gh` config dir of the account bound to `dir`. Errors when the folder is unbound or its
    /// account has no GitHub identity: a push or PR never falls back to the parent's `gh` login.
    pub fn github_config_dir(&self, dir: &Path) -> Result<PathBuf> {
        let account = self
            .resolve(dir)
            .with_context(|| format!("{} is not bound to an account", dir.display()))?;
        check_account(account)?;
        account.gh_config_dir.clone().with_context(|| {
            format!(
                "account '{}' has no GitHub login; set its GitHub config dir in Settings and log in",
                account.id
            )
        })
    }
}

/// Resolves symlinks in the part of `p` that exists and re-appends the rest, so a folder
/// that doesn't exist yet compares like one that does. `None` when `..` follows a missing
/// folder, or when an entry exists but can't be resolved (a broken symlink): that is not "missing".
fn canonical(p: &Path) -> Option<PathBuf> {
    let mut missing = Vec::new();
    let mut cur = p;
    let rejoin = |base: PathBuf, missing: &[&std::ffi::OsStr]| {
        missing.iter().rev().fold(base, |acc, name| acc.join(name))
    };
    loop {
        if let Ok(real) = std::fs::canonicalize(cur) {
            return Some(rejoin(real, &missing));
        }
        // Some Windows volumes can't canonicalize a real folder, so only a link counts as broken.
        if std::fs::symlink_metadata(cur).is_ok_and(|m| m.file_type().is_symlink()) {
            return None;
        }
        let Some(name) = cur.file_name() else {
            // A root that can't be canonicalized (an unplugged Windows drive) is kept as written.
            let root = cur.is_absolute() && !cur.ends_with("..");
            return root.then(|| rejoin(cur.to_path_buf(), &missing));
        };
        missing.push(name);
        cur = cur.parent()?;
    }
}

/// The canonical folder a binding covers. An empty path is a prefix of every folder, and a
/// relative one resolves against the app's cwd, so both are refused.
fn binding_root(p: &Path) -> Result<PathBuf> {
    check_bound_path(p)?;
    canonical(p).with_context(|| format!("can't resolve {}", p.display()))
}

fn check_bound_path(p: &Path) -> Result<()> {
    if !p.is_absolute() || p.components().any(|c| c == std::path::Component::ParentDir) {
        bail!(
            "bound folder must be an absolute path without '..', got '{}'",
            p.display()
        );
    }
    Ok(())
}

/// An empty or relative `CLAUDE_CONFIG_DIR` / `CODEX_HOME` would run the agent under some other account.
fn check_account(a: &Account) -> Result<()> {
    if a.id.trim().is_empty() {
        bail!("account id can't be empty");
    }
    if !a.config_dir.is_absolute() {
        bail!(
            "config dir for '{}' must be an absolute path, got '{}'",
            a.id,
            a.config_dir.display()
        );
    }
    if let Some(gh) = a.gh_config_dir.as_ref().filter(|d| !d.is_absolute()) {
        bail!(
            "GitHub config dir for '{}' must be an absolute path, got '{}'",
            a.id,
            gh.display()
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

#[cfg(test)]
mod linked_envs_test;
#[cfg(test)]
mod recent_envs_test;

#[cfg(test)]
mod onboarding_test;

#[cfg(test)]
mod update_test;

#[cfg(test)]
mod context_test;

#[cfg(test)]
mod update_install_test;

#[cfg(test)]
mod files_save_test;
