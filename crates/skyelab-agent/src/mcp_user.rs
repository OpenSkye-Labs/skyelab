//! The bound account's own MCP servers (Claude Code's user scope), added next to SkyeLab's in the `--mcp-config`
//! file. Never the repo's `.mcp.json` or the account's per-folder `projects.*` servers: a repo must not add tools.
//! Their tools aren't pre-allowed, so each call asks like any other tool.

use anyhow::{Context as _, Result, bail};
use serde_json::{Map, Value};
use std::io::Write as _;
use std::path::Path;

/// The `mcpServers` at the top of `<config_dir>/.claude.json`, where Claude Code keeps user-scope servers. No file or
/// no key: none. Anything else that isn't an object of objects is an error, so the caller can say why it skipped them.
pub(crate) fn claude_servers(config_dir: &Path) -> Result<Map<String, Value>> {
    let path = config_dir.join(".claude.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let v: Value =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let servers = match v.get("mcpServers") {
        _ if !v.is_object() => bail!("{} is not a JSON object", path.display()),
        None | Some(Value::Null) => return Ok(Map::new()),
        Some(Value::Object(servers)) => servers,
        Some(_) => bail!("mcpServers in {} is not an object", path.display()),
    };
    if let Some((name, _)) = servers.iter().find(|(_, s)| !s.is_object()) {
        bail!("MCP server {name:?} in {} is not an object", path.display());
    }
    Ok(servers.clone())
}

/// Whether Claude could read `name`'s tools as SkyeLab's: tool names are `mcp__<server>__<tool>` with the server name's
/// characters outside `[A-Za-z0-9_-]` made `_`, and `--allowedTools mcp__skyelab` pre-allows SkyeLab's. So any name
/// that starts with `skyelab` once normalized (`skyelab__x`, `SkyeLab`, `skyelab.x`) is left out.
fn shadows_skyelab(name: &str) -> bool {
    let normalized: String = name
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' => c.to_ascii_lowercase(),
            _ => '_',
        })
        .collect();
    normalized.starts_with("skyelab")
}

/// Adds `servers` to the `--mcp-config` file at `grant`, minus any named like SkyeLab's own (which must keep its
/// name and its pre-allowed tools). Returns the names added and the names skipped.
pub(crate) fn merge(
    grant: &Path,
    servers: Map<String, Value>,
) -> Result<(Vec<String>, Vec<String>)> {
    if servers.is_empty() {
        return Ok(Default::default());
    }
    let text =
        std::fs::read_to_string(grant).with_context(|| format!("reading {}", grant.display()))?;
    let mut v: Value = serde_json::from_str(&text).context("parsing the MCP grant")?;
    let list = v["mcpServers"]
        .as_object_mut()
        .context("the MCP grant has no mcpServers")?;
    let (mut added, mut skipped) = (Vec::new(), Vec::new());
    for (name, server) in servers {
        if shadows_skyelab(&name) || list.contains_key(&name) {
            skipped.push(name);
            continue;
        }
        list.insert(name.clone(), server);
        added.push(name);
    }
    if added.is_empty() {
        return Ok((added, skipped));
    }
    // Written whole to a new private file, then renamed over the grant: a failed write leaves the grant as it was.
    let text = serde_json::to_vec(&v).context("encoding the MCP config")?;
    let tmp = grant.with_extension("json.new");
    let _ = std::fs::remove_file(&tmp); // a leftover from a crash; create_private_file refuses an existing file
    let written = crate::mcp::create_private_file(&tmp)
        .and_then(|mut file| file.write_all(&text))
        .and_then(|()| std::fs::rename(&tmp, grant));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("writing {}", grant.display()));
    }
    Ok((added, skipped))
}

/// What a new agent process was given, as the first event of its stream (so the details card drops an earlier
/// process's list): SkyeLab's server when there is a grant, and for Claude the account's own servers, merged into it.
/// Those need the grant's file: a launch without one (a side chat) gets none. A config that can't be read or merged,
/// or a server left out, starts without them and says why in `warning`.
pub(crate) fn launch_report(
    claude_config_dir: Option<&Path>,
    grant: Option<&Path>,
) -> crate::Event {
    let mut names: Vec<String> = grant.map(|_| "skyelab".to_owned()).into_iter().collect();
    let mut warning = None;
    if let (Some(dir), Some(grant)) = (claude_config_dir, grant) {
        match claude_servers(dir).and_then(|servers| merge(grant, servers)) {
            Ok((added, skipped)) => {
                names.extend(added);
                if !skipped.is_empty() {
                    warning = Some(format!(
                        "Left out your MCP server{} {}: a name starting with \"skyelab\" is SkyeLab's own.",
                        if skipped.len() == 1 { "" } else { "s" },
                        skipped.join(", ")
                    ));
                }
            }
            Err(e) => {
                warning = Some(format!(
                    "Your account's own MCP servers weren't loaded: {e:#}"
                ))
            }
        }
    }
    crate::Event::Mcp {
        servers: names
            .into_iter()
            .map(|name| crate::McpServer {
                name,
                status: "starting".into(),
                error: None,
            })
            .collect(),
        launch: true,
        warning,
    }
}
