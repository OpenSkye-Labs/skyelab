//! UI-free domain logic. Nothing in this crate may depend on GPUI.

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
        for b in &self.bindings {
            if !self.accounts.iter().any(|a| a.id == b.account) {
                bail!(
                    "binding {} points at unknown account '{}'",
                    b.path.display(),
                    b.account
                );
            }
        }
        Ok(())
    }

    /// Validate, then write atomically (temp file in the same dir, then rename), creating the parent dir.
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let text = toml::to_string_pretty(self).context("serializing accounts config")?;
        let dir = path
            .parent()
            .with_context(|| format!("{} has no parent dir", path.display()))?;
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
    }

    pub fn add_account(&mut self, account: Account) -> Result<()> {
        if account.id.trim().is_empty() {
            bail!("account id can't be empty");
        }
        if self.accounts.iter().any(|a| a.id == account.id) {
            bail!("account '{}' already exists", account.id);
        }
        self.accounts.push(account);
        Ok(())
    }

    /// Replace the label and config dir of the account with the same id.
    pub fn update_account(&mut self, account: Account) -> Result<()> {
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

    pub fn add_binding(&mut self, binding: Binding) -> Result<()> {
        if !self.accounts.iter().any(|a| a.id == binding.account) {
            bail!("unknown account '{}'", binding.account);
        }
        if self.bindings.iter().any(|b| b.path == binding.path) {
            bail!("{} is already bound", binding.path.display());
        }
        self.bindings.push(binding);
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
        let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        self.bindings
            .iter()
            .filter(|b| {
                let bp = std::fs::canonicalize(&b.path).unwrap_or_else(|_| b.path.clone());
                dir.starts_with(bp) // component-wise, so /work/acme does not match /work/acme-old
            })
            .max_by_key(|b| b.path.components().count())
            .and_then(|b| self.accounts.iter().find(|a| a.id == b.account))
    }

    /// Env to launch `claude` with for `dir`. Errors when the folder is unbound.
    pub fn launch_env(&self, dir: &Path) -> Result<Vec<(&'static str, PathBuf)>> {
        let account = self
            .resolve(dir)
            .with_context(|| format!("{} is not bound to an account", dir.display()))?;
        Ok(vec![("CLAUDE_CONFIG_DIR", account.config_dir.clone())])
    }
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
        account = "acme"

        [[binding]]
        path = "/nonexistent/work/acme/oss-fork"
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
    fn unknown_account_is_rejected() {
        let mut c = cfg();
        c.bindings[0].account = "typo".into();
        assert!(c.validate().is_err());
    }
}
