//! What SkyeLab tells `codex app-server` about its tools and a mode's policy.

use crate::Mode;
use anyhow::{Context as _, Result};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The `config` override for SkyeLab's tools, from a `claude --mcp-config` grant file, sent over stdin (no token on a
/// command line). A dotted key sets one entry; a whole `mcp_servers` table would drop the account's own servers.
pub(crate) fn mcp_servers(grant: &Path) -> Result<Value> {
    let text =
        std::fs::read_to_string(grant).with_context(|| format!("reading {}", grant.display()))?;
    let v: Value = serde_json::from_str(&text).context("parsing the MCP grant")?;
    let s = &v["mcpServers"]["skyelab"];
    anyhow::ensure!(s["command"].is_string(), "the MCP grant names no command");
    Ok(
        json!({ "mcp_servers.skyelab": { "command": s["command"], "args": s["args"], "env": s["env"] } }),
    )
}

/// Codex's `approvalPolicy` and `sandbox` for a mode. Plan is also a read-only sandbox (its `plan` collaboration mode
/// goes with each turn, `Codex::pump`).
pub(crate) fn policy(mode: Mode) -> (&'static str, &'static str) {
    match mode {
        // Edits inside the folder never ask under workspace-write, so Accept edits differs only for Claude.
        Mode::Default | Mode::AcceptEdits => ("untrusted", "workspace-write"),
        Mode::Plan => ("on-request", "read-only"),
        Mode::Bypass => ("never", "danger-full-access"),
    }
}

/// `app-server` with every project layer off: `dir`, each ancestor and (for a worktree) its main repo are
/// marked untrusted, so a repo's `.codex/config.toml`, hooks and `.rules` never load. Overrides merge
/// with the user's `projects` table, which is why every ancestor is named, not just `dir`.
pub(crate) fn args(dir: &Path) -> Result<Vec<OsString>> {
    let mut roots: BTreeSet<&Path> = dir.ancestors().collect();
    let main = main_repo(dir);
    roots.extend(main.iter().flat_map(|m| m.ancestors()));
    let entries = roots
        .iter()
        .map(|p| {
            let p = p.to_str().context("folder path is not UTF-8")?;
            // A JSON string is a valid TOML basic string.
            Ok(format!("{}={{trust_level=\"untrusted\"}}", json!(p)))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(vec![
        "app-server".into(),
        "-c".into(),
        format!("projects={{{}}}", entries.join(",")).into(),
    ])
}

/// The main checkout of the git worktree `dir` is in (from its `.git` file and `commondir`), if any.
pub(super) fn main_repo(dir: &Path) -> Option<PathBuf> {
    let top = dir.ancestors().find(|a| a.join(".git").exists())?;
    let text = std::fs::read_to_string(top.join(".git")).ok()?; // a directory: a plain repo, no worktree
    let gitdir = top.join(text.strip_prefix("gitdir:")?.trim());
    let common = std::fs::read_to_string(gitdir.join("commondir")).ok()?;
    let common = std::fs::canonicalize(gitdir.join(common.trim())).ok()?;
    common.parent().map(Path::to_owned)
}
