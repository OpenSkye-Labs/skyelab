//! What a launched `claude` may inherit from the parent: the env it sees and the binary it is.

use anyhow::{Result, bail};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Parent env that passes through; everything else (credentials, providers, base URLs, the
/// parent's own CLAUDE_CONFIG_DIR) is dropped. `LC_*` passes on unix too.
const UNIX_ENV: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "SHELL", "TMPDIR", "LANG", "TERM",
];
const WINDOWS_ENV: &[&str] = &[
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
        WINDOWS_ENV.iter().any(|a| a.eq_ignore_ascii_case(name))
    } else {
        UNIX_ENV.contains(&name) || name.starts_with("LC_")
    }
}

/// The parent env to pass through, with PATH cut down to its absolute entries.
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

#[cfg(windows)]
fn candidates(dir: &Path, bin: &Path) -> Vec<PathBuf> {
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    let mut out = vec![dir.join(bin)];
    if bin.extension().is_none() {
        out.extend(
            exts.split(';')
                .map(|e| dir.join(format!("{}{e}", bin.display()))),
        );
    }
    out
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
