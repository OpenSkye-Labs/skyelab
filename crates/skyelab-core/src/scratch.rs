//! Scratch threads: quick questions outside any project. Each still runs in a folder bound to the account the user
//! picked (`<data dir>/skyelab/scratch/<account>`), so the accounts invariant holds: no default account, ever.

use crate::{AccountsConfig, onboarding::bind_project};
use anyhow::{Context as _, Result, bail};
use std::path::{Path, PathBuf};

/// The scratch folder for `account` under `root`. Ids are free text: a plain one is used as is, anything else hex.
pub fn folder(root: &Path, account: &str) -> PathBuf {
    let plain = !account.is_empty()
        && !account.starts_with('.')
        && account
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    let name = if plain {
        account.to_owned()
    } else {
        account.bytes().map(|b| format!("{b:02x}")).collect()
    };
    root.join(name)
}

/// Creates `account`'s scratch folder under `root` if needed and binds it to `account` (an unknown account, or a
/// folder bound exactly to another account, is an error and nothing changes). Returns the folder.
pub fn prepare(config: &mut AccountsConfig, root: &Path, account: &str) -> Result<PathBuf> {
    if !config.accounts.iter().any(|a| a.id == account) {
        bail!("unknown account '{account}'");
    }
    let dir = folder(root, account);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    // A link here could point the scratch binding at a real project folder.
    if std::fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("{} is a symlink, not a scratch folder", dir.display());
    }
    bind_project(config, &dir, account)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AccountsConfig {
        toml::from_str(
            r#"
            [[account]]
            id = "work"
            label = "Work"
            config_dir = "/nonexistent/.claude-work"

            [[account]]
            id = "lab"
            label = "Codex Lab"
            provider = "codex"
            config_dir = "/nonexistent/.codex"
            "#,
        )
        .expect("parse")
    }

    #[test]
    fn prepare_creates_and_binds_to_the_picked_account_only() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut c = config();
        let dir = prepare(&mut c, root.path(), "lab").expect("prepare");
        assert!(dir.is_dir());
        assert_eq!(dir, root.path().join("lab"));
        assert_eq!(c.resolve(&dir).map(|a| a.id.as_str()), Some("lab"));
        // The other account's folder is not bound by this: it resolves to nothing, never a default.
        let other = folder(root.path(), "work");
        std::fs::create_dir_all(&other).expect("mkdir");
        assert!(c.resolve(&other).is_none());
        // Again: the same folder, no second binding.
        assert_eq!(prepare(&mut c, root.path(), "lab").expect("again"), dir);
        assert_eq!(c.bindings.len(), 1);
        let work = prepare(&mut c, root.path(), "work").expect("work");
        assert_eq!(c.resolve(&work).map(|a| a.id.as_str()), Some("work"));
    }

    #[test]
    fn unknown_account_is_refused_before_any_folder() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut c = config();
        assert!(prepare(&mut c, root.path(), "ghost").is_err());
        assert!(c.bindings.is_empty());
        assert!(!folder(root.path(), "ghost").exists());
    }

    #[test]
    fn folder_bound_to_another_account_stays_bound() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut c = config();
        let dir = prepare(&mut c, root.path(), "work").expect("prepare");
        // Someone rebinds work's scratch folder name to lab by hand: lab's pick must not take it over.
        c.bindings[0].account = "lab".into();
        let err = prepare(&mut c, root.path(), "work").expect_err("claimed");
        assert!(format!("{err:#}").contains("already bound"), "{err:#}");
        assert_eq!(c.resolve(&dir).map(|a| a.id.as_str()), Some("lab"));
    }

    #[test]
    fn odd_ids_become_hex() {
        let root = Path::new("/r");
        assert_eq!(folder(root, "work-1"), Path::new("/r/work-1"));
        assert_eq!(folder(root, "../x"), Path::new("/r/2e2e2f78"));
        assert_eq!(folder(root, ".."), Path::new("/r/2e2e"));
    }
}
