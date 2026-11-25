//! The bound account's own MCP servers (Claude Code's user scope), added next to SkyeLab's in the `--mcp-config`
//! file. Never the repo's `.mcp.json` or the account's per-folder `projects.*` servers: a repo must not add tools.
//! Their tools aren't pre-allowed, so each call asks like any other tool. The account's enabled plugins' servers load
//! the same way, under the names Claude Code gives them (`plugin:<plugin>:<server>`), since `--strict-mcp-config` (still used
//! when the account's config can't be read) drops Claude's own plugin loading.

use anyhow::{Context as _, Result, bail};
use serde_json::{Map, Value};
use std::io::Write as _;
use std::path::{Component, Path};

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

/// Reads a JSON file into an object. Missing file: `None`.
fn read_object(path: &Path) -> Result<Option<Map<String, Value>>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    match serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))? {
        Value::Object(map) => Ok(Some(map)),
        _ => bail!("{} is not a JSON object", path.display()),
    }
}

/// One plugin's servers, from its `plugin.json` `mcpServers` (a path, a list of paths or the servers inline) or else
/// its root `.mcp.json`, named and expanded as Claude Code does.
fn one_plugin(config_dir: &Path, key: &str, root: &Path) -> Result<Map<String, Value>> {
    if !root.is_dir() {
        bail!("its folder {} is missing", root.display());
    }
    let manifest = read_object(&root.join(".claude-plugin/plugin.json"))?.unwrap_or_default();
    let mut servers = Map::new();
    // The default `.mcp.json` may be absent; a file the manifest names must be there.
    let files: Vec<String> = match manifest.get("mcpServers") {
        None => vec![".mcp.json".into()],
        Some(Value::String(path)) => vec![path.clone()],
        Some(Value::Array(paths)) => paths
            .iter()
            .map(|p| {
                p.as_str()
                    .map(str::to_owned)
                    .context("an mcpServers entry isn't a path")
            })
            .collect::<Result<_>>()?,
        Some(Value::Object(inline)) => {
            servers = named(config_dir, key, root, inline.clone());
            Vec::new()
        }
        Some(_) => bail!("its mcpServers is neither a path nor an object"),
    };
    let default = manifest.get("mcpServers").is_none();
    for file in files {
        // Inside the plugin only: a `../` or absolute path could name a repo's `.mcp.json`.
        let inside = Path::new(&file)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
        if !inside {
            bail!("its server file {file} is outside the plugin");
        }
        let Some(v) = read_object(&root.join(&file))? else {
            if default {
                continue;
            }
            bail!("its server file {file} is missing");
        };
        // `.mcp.json` holds `{"mcpServers": {…}}`; a file without that key is the servers map itself.
        let list = match v.get("mcpServers") {
            Some(Value::Object(list)) => list.clone(),
            Some(_) => bail!("mcpServers in {file} is not an object"),
            None => v,
        };
        servers.extend(named(config_dir, key, root, list));
    }
    if let Some((name, _)) = servers.iter().find(|(_, s)| !s.is_object()) {
        bail!("its MCP server {name:?} is not an object");
    }
    Ok(servers)
}

/// `plugin:<plugin>:<server>`, with `${CLAUDE_PLUGIN_ROOT}` and `${CLAUDE_PLUGIN_DATA}` filled in everywhere.
/// shortcut: `${CLAUDE_PLUGIN_OPTION_*}` (plugin user options) stays literal; fill it in when a plugin we use needs it.
fn named(
    config_dir: &Path,
    key: &str,
    root: &Path,
    servers: Map<String, Value>,
) -> Map<String, Value> {
    let plugin = key.split('@').next().unwrap_or(key);
    let data = config_dir.join("plugins/data").join(key.replace('@', "-"));
    let (root, data) = (root.display().to_string(), data.display().to_string());
    fn expand(v: &mut Value, root: &str, data: &str) {
        match v {
            Value::String(s) => {
                *s = s
                    .replace("${CLAUDE_PLUGIN_ROOT}", root)
                    .replace("${CLAUDE_PLUGIN_DATA}", data);
            }
            Value::Array(list) => list.iter_mut().for_each(|v| expand(v, root, data)),
            Value::Object(map) => map.values_mut().for_each(|v| expand(v, root, data)),
            _ => {}
        }
    }
    servers
        .into_iter()
        .map(|(name, mut server)| {
            expand(&mut server, &root, &data);
            (format!("plugin:{plugin}:{name}"), server)
        })
        .collect()
}

/// The MCP servers of the account's enabled plugins: `settings.json` `enabledPlugins` set to `true`, each at its
/// user-scope `installPath` in `plugins/installed_plugins.json` (Claude Code follows that path even into another
/// config dir, and runs that plugin's skills and hooks from it, so it is the account's own choice). Project- and
/// local-scope installs belong to a repo's settings, which never load. A plugin that can't be read is left out and
/// named in the second list.
pub(crate) fn plugin_servers(config_dir: &Path) -> (Map<String, Value>, Vec<String>) {
    let (mut servers, mut problems) = (Map::new(), Vec::new());
    let read = || -> Result<Vec<(String, Option<String>)>> {
        let settings = read_object(&config_dir.join("settings.json"))?.unwrap_or_default();
        let Some(Value::Object(enabled)) = settings.get("enabledPlugins") else {
            return Ok(Vec::new());
        };
        let installed =
            read_object(&config_dir.join("plugins/installed_plugins.json"))?.unwrap_or_default();
        Ok(enabled
            .iter()
            .filter(|(_, on)| on.as_bool() == Some(true))
            .map(|(key, _)| {
                let path = installed
                    .get("plugins")
                    .and_then(|p| p.get(key))
                    .and_then(Value::as_array)
                    .and_then(|list| list.iter().find(|e| e["scope"] == "user"))
                    .and_then(|e| e["installPath"].as_str())
                    .map(str::to_owned);
                (key.clone(), path)
            })
            .collect())
    };
    let plugins = match read() {
        Ok(plugins) => plugins,
        Err(e) => return (servers, vec![format!("{e:#}")]),
    };
    for (key, path) in plugins {
        let Some(path) = path else { continue }; // not installed for the user scope
        let root = Path::new(&path);
        if !root.is_absolute() {
            problems.push(format!("{key}: its install path isn't absolute"));
            continue;
        }
        // Claude Code installs every plugin into a `plugins/cache` (this config dir's or another's). Anywhere else
        // could be a checkout an agent can write, which would let it add servers to its own next launch.
        let parts: Vec<_> = root.components().map(|c| c.as_os_str()).collect();
        if !parts
            .windows(2)
            .any(|w| w[0] == "plugins" && w[1] == "cache")
        {
            problems.push(format!(
                "{key}: it isn't installed in a plugins/cache folder"
            ));
            continue;
        }
        match one_plugin(config_dir, &key, root) {
            Ok(list) => {
                for (name, server) in list {
                    if servers.contains_key(&name) {
                        problems.push(format!("{key}: another plugin already has {name}"));
                    } else {
                        servers.insert(name, server);
                    }
                }
            }
            Err(e) => problems.push(format!("{key}: {e:#}")),
        }
    }
    (servers, problems)
}

/// What a new agent process was given, as the first event of its stream (so the details card drops an earlier
/// process's list): SkyeLab's server when there is a grant, and for Claude the account's own servers, merged into it.
/// Those need the grant's file: a launch without one (a side chat) gets none. A config that can't be read or merged,
/// or a server left out, starts without them and says why in `warning`.
///
/// Also the user-scope names Claude must be told to refuse (`deniedMcpServers`) when it loads the account's config
/// and claude.ai connectors itself, or `None` to keep `--strict-mcp-config`: no grant, not Claude, or a config we
/// couldn't read, so we can't tell which names it would load.
pub(crate) fn launch_report(
    claude_config_dir: Option<&Path>,
    grant: Option<&Path>,
) -> (crate::Event, Option<Vec<String>>) {
    let mut names: Vec<String> = grant.map(|_| "skyelab".to_owned()).into_iter().collect();
    let mut notes = Vec::new();
    let mut deny = None;
    if let (Some(dir), Some(grant)) = (claude_config_dir, grant) {
        let mut read_ok = true;
        let mut servers = claude_servers(dir).unwrap_or_else(|e| {
            read_ok = false;
            notes.push(format!(
                "Your account's own MCP servers weren't loaded: {e:#}"
            ));
            Map::new()
        });
        let (plugins, problems) = plugin_servers(dir);
        if !problems.is_empty() {
            notes.push(format!(
                "Plugin MCP servers left out: {}.",
                problems.join("; ")
            ));
        }
        let mut clashes = Vec::new();
        for (name, server) in plugins {
            if servers.contains_key(&name) {
                clashes.push(name);
            } else {
                servers.insert(name, server);
            }
        }
        if !clashes.is_empty() {
            notes.push(format!(
                "Left out plugin MCP server{} {}: your own server has that name.",
                if clashes.len() == 1 { "" } else { "s" },
                clashes.join(", ")
            ));
        }
        match merge(grant, servers) {
            Ok((added, skipped)) => {
                names.extend(added);
                // Exactly `skyelab` is replaced by ours (`--mcp-config` wins a name); denying it would refuse ours too.
                deny = read_ok.then(|| {
                    skipped
                        .iter()
                        .filter(|n| *n != "skyelab")
                        .cloned()
                        .collect()
                });
                if !skipped.is_empty() {
                    notes.push(format!(
                        "Left out your MCP server{} {}: a name starting with \"skyelab\" is SkyeLab's own.",
                        if skipped.len() == 1 { "" } else { "s" },
                        skipped.join(", ")
                    ));
                }
            }
            Err(e) => notes.push(format!(
                "Your account's own MCP servers weren't loaded: {e:#}"
            )),
        }
    }
    let warning = (!notes.is_empty()).then(|| notes.join(" "));
    let report = crate::Event::Mcp {
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
    };
    (report, deny)
}
