//! The environment the phone dials: one MagicDNS name, remembered in the app's data directory.

use anyhow::Context as _;
use std::path::Path;

pub const FILE: &str = "environment.txt";

/// The name, trimmed and lowercased, if it is a MagicDNS name (`laptop.tail1234.ts.net`), the same rule as the
/// desktop picker (`skyelab_protocol::is_magicdns_name`). No scheme, port or short name: TLS certificates exist
/// only for MagicDNS names, and the client builds the URL itself (docs/mobile.md step 4).
pub fn validate(name: &str) -> Result<String, &'static str> {
    let name = name.trim().to_ascii_lowercase();
    if name.is_empty() {
        return Err("Enter the machine's MagicDNS name.");
    }
    if !skyelab_protocol::is_magicdns_name(&name) {
        return Err(
            "Use the machine's MagicDNS name, like laptop.tail1234.ts.net, without ws:// or a port.",
        );
    }
    Ok(name)
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
