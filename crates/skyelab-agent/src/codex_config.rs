//! What SkyeLab tells `codex app-server` about its tools and a mode's policy.

use crate::Mode;
use anyhow::{Context as _, Result};
use serde_json::{Value, json};
use std::path::Path;

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

/// Codex's `approvalPolicy` and `sandbox` for a mode. Codex has no plan mode, so Plan is a read-only sandbox.
pub(crate) fn policy(mode: Mode) -> (&'static str, &'static str) {
    match mode {
        // Edits inside the folder never ask under workspace-write, so Accept edits differs only for Claude.
        Mode::Default | Mode::AcceptEdits => ("untrusted", "workspace-write"),
        Mode::Plan => ("on-request", "read-only"),
        Mode::Bypass => ("never", "danger-full-access"),
    }
}
