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

/// [`write_atomic`] to exactly `target` (already resolved: it isn't resolved again). The temp file next to it gets a
/// random name and is created exclusively (never through a link planted there), owner-only until it takes an
/// existing file's permissions; a new file stays owner-only. A crash can leave a `.<name>.*.tmp` behind.
pub(crate) fn write_atomic_at(target: &Path, bytes: &[u8]) -> Result<()> {
    let dir = target
        .parent()
        .with_context(|| format!("{} has no parent dir", target.display()))?;
    let name = target
        .file_name()
        .with_context(|| format!("{} has no file name", target.display()))?;
    let perms = std::fs::metadata(target).ok().map(|m| m.permissions());
    let written = (|| {
        let mut tmp = tempfile::Builder::new()
            .prefix(&format!(".{}.", name.to_string_lossy()))
            .suffix(".tmp")
            .tempfile_in(dir)?;
        std::io::Write::write_all(&mut tmp, bytes)?;
        if let Some(perms) = perms {
            tmp.as_file().set_permissions(perms)?;
        }
        tmp.as_file().sync_all()?;
        // A failed persist drops the temp file.
        tmp.persist(target).map_err(|e| e.error)?;
        anyhow::Ok(())
    })();
    written.with_context(|| format!("saving {}", target.display()))
}

#[cfg(all(test, unix))]
#[test]
fn a_new_file_is_private_and_an_old_one_keeps_its_mode() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().expect("temp dir");
    let new = dir.path().join("accounts.toml");
    write_atomic(&new, "a = 1\n").expect("write");
    let mode = |p: &Path| std::fs::metadata(p).expect("stat").permissions().mode() & 0o777;
    assert_eq!(mode(&new), 0o600, "a new config is owner-only");
    std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o640)).expect("chmod");
    write_atomic(&new, "a = 2\n").expect("rewrite");
    assert_eq!(mode(&new), 0o640, "an existing file keeps its mode");
    assert_eq!(std::fs::read_to_string(&new).expect("read"), "a = 2\n");
    assert_eq!(
        std::fs::read_dir(dir.path()).expect("list").count(),
        1,
        "no temp file left"
    );
}
