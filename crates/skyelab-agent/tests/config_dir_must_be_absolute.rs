#![cfg(unix)]
use skyelab_agent::start_with_bin;
use skyelab_core::{Account, AccountsConfig, Binding};
use std::path::{Path, PathBuf};

fn cfg(dir: &Path, config_dir: &str) -> AccountsConfig {
    AccountsConfig {
        accounts: vec![Account {
            id: "a".into(),
            label: "A".into(),
            config_dir: config_dir.into(),
        }],
        bindings: vec![Binding {
            path: dir.into(),
            account: "a".into(),
        }],
    }
}

/// A relative config dir would resolve inside the untrusted project; an empty one falls back to ~/.claude.
#[test]
fn relative_or_empty_config_dir_is_refused() {
    let dir = std::env::temp_dir().join(format!("skyelab-cfgdir-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let dir = std::fs::canonicalize(&dir).expect("canon");
    let bin: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    for bad in ["~/.claude-acme", "acct", ""] {
        let res = start_with_bin(&bin, &cfg(&dir, bad), &dir, "hi");
        assert!(res.is_err(), "CONFIG_DIR_NOT_REFUSED: {bad:?}");
    }
}
