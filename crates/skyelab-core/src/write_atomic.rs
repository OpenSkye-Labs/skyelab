//! Crash-safe file writes.

use anyhow::{Context as _, Result};
use std::path::Path;

/// Writes `text` to `path` through a temp file and a rename, so a crash never leaves half a file.
pub(crate) fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let dir = path
        .parent()
        .with_context(|| format!("{} has no parent dir", path.display()))?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    // Write through a symlinked config (dotfiles) instead of replacing the link.
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    write_atomic_at(&target, text.as_bytes())
}

/// [`write_atomic`] to exactly `target` (already resolved: it is not resolved again, so a symlink planted there
/// since is replaced, never followed). An existing file's permissions carry over.
pub(crate) fn write_atomic_at(target: &Path, bytes: &[u8]) -> Result<()> {
    let name = target
        .file_name()
        .with_context(|| format!("{} has no file name", target.display()))?;
    // Unique per process, so two running apps never interleave writes into one temp file.
    let tmp = target.with_file_name(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let perms = std::fs::metadata(target).ok().map(|m| m.permissions());
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        std::io::Write::write_all(&mut f, bytes)?;
        if let Some(perms) = perms {
            f.set_permissions(perms)?;
        }
        f.sync_all()?;
        std::fs::rename(&tmp, target)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp); // best effort; the original file is untouched
    }
    written.with_context(|| format!("saving {}", target.display()))
}
