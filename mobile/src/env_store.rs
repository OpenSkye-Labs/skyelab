//! The environment the phone dials: one MagicDNS name, remembered in the app's data directory.

use anyhow::Context as _;
use std::path::Path;

pub const FILE: &str = "environment.txt";

/// The name, trimmed and lowercased, if it is a MagicDNS name: `<machine>.<tailnet>.ts.net`. Raw IPs, `localhost`
/// and short names are refused: TLS certificates exist only for MagicDNS names (docs/architecture.md, Decisions).
/// No scheme, port or spaces: the client builds the URL itself.
pub fn validate(name: &str) -> Result<String, &'static str> {
    const FORM: &str = "Use the machine's MagicDNS name, <machine>.<tailnet>.ts.net (Tailscale shows it), not an \
                        IP address, localhost or a short name.";
    let name = name.trim().to_ascii_lowercase();
    if name.is_empty() {
        return Err("Enter the machine's MagicDNS name, <machine>.<tailnet>.ts.net.");
    }
    let label_ok = |l: &str| {
        (1..=63).contains(&l.len())
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    let labels: Vec<&str> = name.split('.').collect();
    match labels.as_slice() {
        [machine, tailnet, "ts", "net"] if label_ok(machine) && label_ok(tailnet) => Ok(name),
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
