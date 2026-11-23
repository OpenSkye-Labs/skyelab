//! Which binary a launched agent is. The env it inherits is `skyelab_core::env`'s rule.

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

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
    skyelab_core::env::absolute_entries(&path)
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
