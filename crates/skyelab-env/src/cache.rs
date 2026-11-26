//! The last shell snapshot of each remote machine, kept on disk so its threads show at launch before the connection
//! is up. One owner-only file per MagicDNS name, read back only for the node that name is pinned to.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use skyelab_protocol::{Project, Thread};

/// The payload's version. A file of another one is ignored (and overwritten by the next save).
pub const SCHEMA: u32 = 1;
/// The largest file read back: protocol 1.7's snapshot cap. A bigger one is ignored.
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cached {
    pub schema: u32,
    pub host: String,
    /// The Tailscale StableID the rows came from.
    pub node_id: String,
    pub saved_at_ms: i64,
    pub projects: Vec<Project>,
    pub threads: Vec<Thread>,
}

impl Cached {
    pub fn new(host: &str, node_id: &str, projects: Vec<Project>, threads: Vec<Thread>) -> Self {
        let saved_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        Self {
            schema: SCHEMA,
            host: host.trim().to_ascii_lowercase(),
            node_id: node_id.to_owned(),
            saved_at_ms,
            projects,
            threads,
        }
    }
}

/// `<cache dir>/<APP_DIR>/remote-shells`, `None` when the platform has no cache dir.
pub fn default_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| {
        d.join(skyelab_core::instance::APP_DIR)
            .join("remote-shells")
    })
}

/// The file for `host`: a hash, so no name can reach outside `dir`.
pub fn file(dir: &Path, host: &str) -> PathBuf {
    let host = host.trim().to_ascii_lowercase();
    let digest = ring::digest::digest(&ring::digest::SHA256, host.as_bytes());
    let hex: String = digest.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    dir.join(format!("{hex}.json"))
}

/// `host`'s rows when they were saved from `expected_node`. Anything else (no pin, another node, a missing, broken,
/// oversized or other-schema file) is `None`, never an error: a cache must not stop the app.
pub fn load(dir: &Path, host: &str, expected_node: Option<&str>) -> Option<Cached> {
    let expected = expected_node?;
    let mut bytes = Vec::new();
    std::fs::File::open(file(dir, host))
        .ok()?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    let cached: Cached = serde_json::from_slice(&bytes).ok()?;
    let ok = cached.schema == SCHEMA
        && cached.host.eq_ignore_ascii_case(host.trim())
        && cached.node_id == expected;
    ok.then_some(cached)
}

/// Writes `cached` through a temp file and a rename. The dir is owner-only (0700), the file too (0600).
pub fn save(dir: &Path, cached: &Cached) -> Result<()> {
    create_private_dir(dir)?;
    let bytes = serde_json::to_vec(cached)?;
    // tempfile creates it 0600 with a random name, exclusively.
    let mut tmp = tempfile::Builder::new()
        .prefix(".shell.")
        .suffix(".tmp")
        .tempfile_in(dir)
        .with_context(|| format!("creating a temp file in {}", dir.display()))?;
    std::io::Write::write_all(&mut tmp, &bytes)?;
    tmp.as_file().sync_all()?;
    let target = file(dir, &cached.host);
    tmp.persist(&target)
        .map_err(|e| e.error)
        .with_context(|| format!("saving {}", target.display()))?;
    Ok(())
}

/// Removes `host`'s file; a missing one is fine.
pub fn delete(dir: &Path, host: &str) -> Result<()> {
    let path = file(dir, host);
    match std::fs::remove_file(&path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("removing {}", path.display()))
        }
        _ => Ok(()),
    }
}

fn create_private_dir(dir: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(dir)
        .with_context(|| format!("creating {}", dir.display()))
}

#[cfg(test)]
#[path = "cache_test.rs"]
mod tests;
