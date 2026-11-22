//! What a launched `claude` may inherit from the parent: the env it sees and the binary it is.

use anyhow::{Result, bail};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Parent env that passes through; everything else (credentials, providers, base URLs, the
/// parent's own CLAUDE_CONFIG_DIR) is dropped. `LC_*` passes on unix too.
const UNIX_ENV: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "SHELL", "TMPDIR", "LANG", "TERM",
];
/// Network and session plumbing from the user's own (trusted) env, on every OS. None of these
/// choose the account or provider; without them agents fail behind proxies or on `git push`.
const SHARED_ENV: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "NODE_EXTRA_CA_CERTS",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "SSH_AUTH_SOCK",
    "TZ",
];
const WINDOWS_ENV: &[&str] = &[
    "SystemDrive",
    "WINDIR",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramData",
    "HOMEDRIVE",
    "HOMEPATH",
    "CLAUDE_CODE_GIT_BASH_PATH",
    "PATH",
    "SystemRoot",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "PATHEXT",
    "COMSPEC",
    "TEMP",
    "TMP",
];

/// Whether the parent's `name` may reach the child. Windows env names are case-insensitive.
pub(crate) fn allowed(name: &OsStr, windows: bool) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    if windows {
        WINDOWS_ENV
            .iter()
            .chain(SHARED_ENV)
            .any(|a| a.eq_ignore_ascii_case(name))
    } else {
        UNIX_ENV.contains(&name)
            || name.starts_with("LC_")
            || name.starts_with("XDG_")
            // unix tools read both spellings of the proxy vars
            || SHARED_ENV.iter().any(|a| *a == name || a.to_ascii_lowercase() == name)
    }
}

pub(crate) fn inherited_env() -> Vec<(OsString, OsString)> {
    std::env::vars_os()
        .filter(|(k, _)| allowed(k, cfg!(windows)))
        .filter_map(|(k, v)| {
            if !k.eq_ignore_ascii_case("PATH") {
                return Some((k, v));
            }
            std::env::join_paths(absolute_entries(&v))
                .ok()
                .map(|p| (k, p))
        })
        .collect()
}

fn absolute_entries(path: &OsStr) -> Vec<PathBuf> {
    std::env::split_paths(path)
        .filter(|p| p.is_absolute())
        .collect()
}

/// Turns `bin` into an absolute path before the child's cwd (an untrusted project) can matter.
/// A bare name is looked up through the parent PATH's absolute entries only.
pub(crate) fn resolve_bin(bin: &Path) -> Result<PathBuf> {
    if bin.is_absolute() {
        return Ok(bin.to_owned());
    }
    if bin.components().count() != 1 {
        bail!("{} must be an absolute path or a bare name", bin.display());
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    absolute_entries(&path)
        .iter()
        .flat_map(|dir| candidates(dir, bin))
        .find(|p| is_executable(p))
        .ok_or_else(|| anyhow::anyhow!("{} not found in PATH", bin.display()))
}

#[cfg(unix)]
fn candidates(dir: &Path, bin: &Path) -> Vec<PathBuf> {
    vec![dir.join(bin)]
}

/// Like Windows itself: a name without an extension only matches with a PATHEXT one, so npm's
/// extensionless sh shim next to `claude.cmd` is never picked.
#[cfg(windows)]
fn candidates(dir: &Path, bin: &Path) -> Vec<PathBuf> {
    if bin.extension().is_some() {
        return vec![dir.join(bin)];
    }
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    exts.split(';')
        .filter(|e| !e.is_empty())
        .map(|e| dir.join(format!("{}{e}", bin.display())))
        .collect()
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    p.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

#[cfg(test)]
mod tests {
    use super::allowed;
    use std::ffi::OsStr;

    fn ok(name: &str, windows: bool) -> bool {
        allowed(OsStr::new(name), windows)
    }

    #[test]
    fn unix_allowlist() {
        for n in [
            "PATH", "HOME", "TMPDIR", "LANG", "LC_CTYPE", "LC_ALL", "TERM",
        ] {
            assert!(ok(n, false), "{n}");
        }
        for n in [
            "Path",
            "CLAUDE_CONFIG_DIR",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_FOUNDRY_API_KEY",
            "SystemRoot",
        ] {
            assert!(!ok(n, false), "{n}");
        }
    }

    /// Network and session plumbing from the user's own env must reach the agent (proxies,
    /// corporate CAs, ssh-agent for `git push`), but never anything that picks the account.
    #[test]
    fn network_and_session_vars_pass_auth_vars_do_not() {
        for windows in [false, true] {
            for n in [
                "HTTPS_PROXY",
                "HTTP_PROXY",
                "NO_PROXY",
                "ALL_PROXY",
                "NODE_EXTRA_CA_CERTS",
                "SSL_CERT_FILE",
                "SSL_CERT_DIR",
                "SSH_AUTH_SOCK",
                "TZ",
            ] {
                assert!(ok(n, windows), "{n} windows={windows}");
            }
            for n in [
                "ANTHROPIC_BASE_URL",
                "CLAUDE_CODE_USE_BEDROCK",
                "CLAUDE_CODE_OAUTH_TOKEN",
            ] {
                assert!(!ok(n, windows), "{n} windows={windows}");
            }
        }
        for n in ["https_proxy", "no_proxy", "XDG_CONFIG_HOME", "XDG_RUNTIME_DIR"] {
            assert!(ok(n, false), "{n}");
        }
        for n in [
            "SystemDrive",
            "WINDIR",
            "ProgramFiles",
            "ProgramFiles(x86)",
            "ProgramData",
            "HOMEDRIVE",
            "HOMEPATH",
            "CLAUDE_CODE_GIT_BASH_PATH",
        ] {
            assert!(ok(n, true), "{n}");
        }
    }

    #[test]
    fn windows_allowlist_ignores_case() {
        for n in [
            "Path",
            "PATH",
            "pathext",
            "SystemRoot",
            "SYSTEMROOT",
            "UserProfile",
            "TEMP",
        ] {
            assert!(ok(n, true), "{n}");
        }
        for n in [
            "CLAUDE_CONFIG_DIR",
            "ANTHROPIC_AUTH_TOKEN",
            "HOME",
            "LC_ALL",
            "claude_config_dir",
        ] {
            assert!(!ok(n, true), "{n}");
        }
    }
}
