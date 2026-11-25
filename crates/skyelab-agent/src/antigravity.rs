//! Antigravity-specific launch configuration on top of the reusable ACP driver (`acp.rs`).
//!
//! Centralizes the isolation env vars and CLI flags so every Antigravity spawn (`start_with_bin`,
//! `login_status`, `list_antigravity_models`, `one_shot`, and `agent_login_options`) keeps OAuth
//! credentials inside the bound account's `GEMINI_HOME` rather than the shared OS keyring.

use crate::acp::AcpConfig;
use anyhow::{Context as _, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Subdirectory passed to `--app-data-dir` beneath `--gemini-dir`.
pub const APP_DATA_DIR: &str = "antigravity-acp";

/// Extra isolation environment variables applied to every Antigravity process and login terminal.
/// `SSH_CONNECTION` + `AGY_ACP_FORCE_FILE_STORAGE=1` keep `agy` 1.5 tokens in file storage under
/// `GEMINI_HOME` instead of a shared OS keyring entry across accounts.
pub fn isolated_env() -> [(&'static str, &'static str); 3] {
    [
        ("AGY_ACP_FORCE_FILE_STORAGE", "1"),
        ("AGY_CLI_DISABLE_AUTO_UPDATE", "true"),
        ("SSH_CONNECTION", "127.0.0.1 0 127.0.0.1 0"),
    ]
}

/// CLI arguments to run `agy acp` bound to `config_dir`.
pub fn acp_args(config_dir: &Path, bypass: bool) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "acp".into(),
        "--gemini-dir".into(),
        config_dir.as_os_str().to_owned(),
        "--app-data-dir".into(),
        APP_DATA_DIR.into(),
    ];
    if bypass {
        args.push("--dangerously-skip-permissions".into());
    }
    args
}

/// CLI arguments to run interactive `agy acp --login` bound to `config_dir`.
pub fn login_args(config_dir: &Path) -> Result<Vec<String>> {
    let dir = config_dir
        .to_str()
        .with_context(|| format!("{} is not valid UTF-8", config_dir.display()))?;
    Ok(vec![
        "acp".into(),
        "--gemini-dir".into(),
        dir.to_owned(),
        "--app-data-dir".into(),
        APP_DATA_DIR.into(),
        "--login".into(),
    ])
}

/// Creates `config_dir` and `config_dir/antigravity-acp` with private permissions on Unix.
pub fn ensure_profile_dirs(config_dir: &Path) -> Result<()> {
    for dir in [config_dir.to_path_buf(), config_dir.join(APP_DATA_DIR)] {
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .with_context(|| format!("making {} private", dir.display()))?;
        }
    }
    Ok(())
}

/// Resolves the default Antigravity CLI binary (`agy`, falling back to `antigravity-acp`).
pub fn default_bin() -> PathBuf {
    for name in ["agy", "antigravity-acp"] {
        if let Ok(found) = crate::launch::resolve_bin(Path::new(name)) {
            return found;
        }
    }
    PathBuf::from("agy")
}

pub(crate) fn config() -> AcpConfig {
    AcpConfig {
        client_name: "skyelab",
        client_version: env!("CARGO_PKG_VERSION"),
        provider_label: "Antigravity",
    }
}
