//! Crash-safe config writes.

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
