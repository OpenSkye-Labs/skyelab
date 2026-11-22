//! UI-free domain logic. Nothing in this crate may depend on GPUI.

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

mod app_settings;
pub use app_settings::AppSettings;

/// One provider login. For Claude Code an account *is* its config dir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub label: String,
    pub config_dir: PathBuf,
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
        dirs::config_dir().map(|d| d.join("skyelab").join("accounts.toml"))
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
        self.accounts.push(account);
        Ok(())
    }

    /// Replace the label and config dir of the account with the same id.
    pub fn update_account(&mut self, account: Account) -> Result<()> {
        check_account(&account)?;
        let slot = self
            .accounts
            .iter_mut()
            .find(|a| a.id == account.id)
            .with_context(|| format!("unknown account '{}'", account.id))?;
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
        self.accounts.iter().find(|a| a.id == b.account)
    }

    /// Env to launch `claude` with for `dir`. Errors when the folder is unbound.
    pub fn launch_env(&self, dir: &Path) -> Result<Vec<(&'static str, PathBuf)>> {
        let account = self
            .resolve(dir)
            .with_context(|| format!("{} is not bound to an account", dir.display()))?;
        check_account(account)?; // the fields are public, so this config may never have been validated
        Ok(vec![("CLAUDE_CONFIG_DIR", account.config_dir.clone())])
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

/// An empty or relative `CLAUDE_CONFIG_DIR` would run the agent under some other account.
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
    Ok(())
}

/// Writes `text` to `path` through a temp file and a rename, so a crash never leaves half a file.
fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let dir = path
        .parent()
        .with_context(|| format!("{} has no parent dir", path.display()))?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    // Write through a symlinked config (dotfiles) instead of replacing the link.
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    // Unique per process, so two running apps never interleave writes into one temp file.
    let tmp = target.with_extension(format!("toml.{}.tmp", std::process::id()));
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        std::io::Write::write_all(&mut f, text.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, &target)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp); // best effort; the original file is untouched
    }
    written.with_context(|| format!("saving {}", target.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: &str = r#"
        [[account]]
        id = "acme"
        label = "Acme (Team)"
        config_dir = "/home/me/.claude-acme"

        [[account]]
        id = "personal"
        label = "Personal (Max)"
        config_dir = "/home/me/.claude"

        [[binding]]
        path = "/nonexistent/work/acme"
        resolved = "/nonexistent/work/acme"
        account = "acme"

        [[binding]]
        path = "/nonexistent/work/acme/oss-fork"
        resolved = "/nonexistent/work/acme/oss-fork"
        account = "personal"
    "#;

    fn cfg() -> AccountsConfig {
        let c: AccountsConfig = toml::from_str(CFG).expect("parse");
        c.validate().expect("valid");
        c
    }

    fn resolved(dir: &str) -> Option<String> {
        cfg().resolve(Path::new(dir)).map(|a| a.id.clone())
    }

    #[test]
    fn deepest_binding_wins() {
        assert_eq!(
            resolved("/nonexistent/work/acme/api").as_deref(),
            Some("acme")
        );
        assert_eq!(
            resolved("/nonexistent/work/acme/oss-fork/src").as_deref(),
            Some("personal")
        );
    }

    #[test]
    fn prefix_is_component_wise_and_unbound_is_none() {
        assert_eq!(resolved("/nonexistent/work/acme-old"), None);
        assert_eq!(resolved("/nonexistent/elsewhere"), None);
        assert!(
            cfg()
                .launch_env(Path::new("/nonexistent/elsewhere"))
                .is_err()
        );
    }

    #[test]
    fn launch_env_refuses_a_relative_config_dir_even_unvalidated() {
        let mut c = cfg();
        c.accounts[0].config_dir = ".claude".into();
        assert!(c.launch_env(Path::new("/nonexistent/work/acme")).is_err());
    }

    #[test]
    fn relative_dir_and_dotdot_binding_never_resolve() {
        assert_eq!(resolved("nonexistent/work/acme"), None);
        let mut c = cfg();
        c.bindings[0].path = "/nonexistent/x/../work/acme".into();
        assert!(c.validate().is_err());
        assert!(c.resolve(Path::new("/nonexistent/work/acme")).is_none());
    }

    #[test]
    fn unknown_account_is_rejected() {
        let mut c = cfg();
        c.bindings[0].account = "typo".into();
        assert!(c.validate().is_err());
    }
}
