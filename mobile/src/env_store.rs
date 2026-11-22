//! The environment the phone dials: one MagicDNS name, remembered in the app's data directory.

use anyhow::Context as _;
use std::collections::BTreeMap;
use std::path::Path;

pub const FILE: &str = "environment.txt";

/// The name, trimmed and lowercased, if it is a MagicDNS name (`laptop.tail1234.ts.net`), the same rule as the
/// desktop picker (`skyelab_protocol::is_magicdns_name`). No scheme, port or short name: TLS certificates exist
/// only for MagicDNS names, and the client builds the URL itself (docs/mobile.md step 4).
pub fn validate(name: &str) -> Result<String, &'static str> {
    const FORM: &str = "Use the machine's MagicDNS name, <machine>.<tailnet>.ts.net (Tailscale shows it), not an \
                        IP address, localhost or a short name.";
    let name = name.trim().to_ascii_lowercase();
    if name.is_empty() {
        return Err("Enter the machine's MagicDNS name, <machine>.<tailnet>.ts.net.");
    }
    let labels: Vec<&str> = name.split('.').collect();
    match () {
        // One rule for desktop and mobile.
        _ if skyelab_protocol::is_magicdns_name(&name) => Ok(name),
        // IPs (v4, v6, bracketed), localhost and dotless or local names all get the same pointer to the right form.
        _ if name.parse::<std::net::IpAddr>().is_ok()
            || name.starts_with('[')
            || labels.len() < 3
            || name.starts_with("localhost")
            || name.ends_with(".local") =>
        {
            Err(FORM)
        }
        _ => Err(
            "That isn't a MagicDNS name. Use <machine>.<tailnet>.ts.net, without ws:// or a port.",
        ),
    }
}

/// The remembered name, or `None` when there is none or the file no longer holds a valid one.
pub fn load(dir: &Path) -> Option<String> {
    validate(&std::fs::read_to_string(dir.join(FILE)).ok()?).ok()
}

pub fn save(dir: &Path, name: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(FILE);
    std::fs::write(&path, format!("{name}\n"))
        .with_context(|| format!("writing {}", path.display()))
}

/// Where each name's pinned node lives: a JSON map, MagicDNS name → the node StableID its hello named when trusted.
pub const NODES_FILE: &str = "nodes.json";

/// The pinned nodes. A missing file is none pinned; any other read or parse failure is an error, so the caller
/// refuses rather than trust (and re-pin) whatever answers.
pub fn load_nodes(dir: &Path) -> anyhow::Result<BTreeMap<String, String>> {
    let path = dir.join(NODES_FILE);
    match std::fs::read_to_string(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        Ok(text) => {
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
        }
    }
}

/// Pins `name` to `node_id`, keeping the other names. Fails (and pins nothing) if the file can't be read.
pub fn trust_node(dir: &Path, name: &str, node_id: &str) -> anyhow::Result<()> {
    let mut nodes = load_nodes(dir)?;
    nodes.insert(name.to_owned(), node_id.to_owned());
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(NODES_FILE);
    // Written beside it, then renamed over it, so a crash mid-write can't leave a file that refuses every name.
    let tmp = dir.join(format!("{NODES_FILE}.tmp"));
    let text = serde_json::to_string_pretty(&nodes).context("serializing pins")?;
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("replacing {}", path.display()))
}
