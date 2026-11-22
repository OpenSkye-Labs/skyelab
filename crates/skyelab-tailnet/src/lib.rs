//! The user's own devices on their tailnet, and whether an incoming peer is one of them.
//!
//! No GPUI here: the settings UI calls [`Tailnet`] and matches on [`Error`].

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Deserializer};

/// Tailscale's numeric user id (`tailcfg.UserID`). Go's zero value means "none".
pub type UserId = u64;

/// A device on the tailnet, as `tailscale status --json` reports it (`Self` and each `Peer`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Node {
    #[serde(rename = "ID", default)]
    pub stable_id: String,
    #[serde(rename = "HostName", default)]
    pub hostname: String,
    #[serde(rename = "DNSName", default)]
    pub dns_name: String,
    #[serde(rename = "TailscaleIPs", default, deserialize_with = "null_as_default")]
    pub tailscale_ips: Vec<IpAddr>,
    #[serde(rename = "OS", default)]
    pub os: String,
    #[serde(rename = "Online", default)]
    pub online: bool,
    #[serde(rename = "UserID", default)]
    pub user_id: Option<UserId>,
    #[serde(rename = "Tags", default, deserialize_with = "null_as_default")]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct UserProfile {
    #[serde(rename = "ID", default)]
    pub id: Option<UserId>,
    #[serde(rename = "LoginName", default)]
    pub login_name: String,
    #[serde(rename = "DisplayName", default)]
    pub display_name: String,
}

/// The node part of a whois answer (`tailcfg.Node`, a different shape from [`Node`]).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WhoIsNode {
    #[serde(rename = "StableID", default)]
    pub stable_id: String,
    #[serde(rename = "Name", default)]
    pub name: String,
    /// The node's owner. For a tagged node this is whoever tagged it, so it never proves ownership alone.
    #[serde(rename = "User", default)]
    pub user: Option<UserId>,
    #[serde(rename = "Tags", default, deserialize_with = "null_as_default")]
    pub tags: Vec<String>,
}

/// Who is behind a tailnet address (`tailscale whois --json`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WhoIs {
    #[serde(rename = "Node")]
    pub node: WhoIsNode,
    #[serde(rename = "UserProfile", default)]
    pub user_profile: UserProfile,
}

/// This device and its peers, from a running, logged-in Tailscale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub self_node: Node,
    pub peers: Vec<Node>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Tailscale is not installed")]
    NotInstalled,
    /// Holds the backend state (`Stopped`, `NeedsLogin`, …) or the CLI's message when the daemon is unreachable.
    #[error("Tailscale is not running or not logged in ({0})")]
    NotRunning(String),
    #[error("`tailscale {command}` failed: {stderr}")]
    Command {
        command: &'static str,
        stderr: String,
    },
    #[error("could not parse Tailscale's output")]
    Parse(#[from] serde_json::Error),
    #[error("could not run tailscale")]
    Io(#[from] std::io::Error),
}

pub trait Tailnet {
    fn self_node(&self) -> Result<Node, Error>;
    fn peers(&self) -> Result<Vec<Node>, Error>;
    fn whois(&self, addr: SocketAddr) -> Result<WhoIs, Error>;
}

/// The security core: accept `whois` only if it is one of the user's own, untagged devices.
///
/// Tagged nodes belong to the tailnet, not a person, so they are rejected even when the same user tagged them.
pub fn is_own_device(self_node: &Node, whois: &WhoIs) -> bool {
    let Some(me) = self_node.user_id.filter(|&id| id != 0) else {
        return false;
    };
    whois.user_profile.id == Some(me) && whois.node.user == Some(me) && whois.node.tags.is_empty()
}

/// Parses `tailscale status --json`. Anything but `BackendState: Running` is [`Error::NotRunning`].
pub fn parse_status(json: &[u8]) -> Result<Status, Error> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(rename = "BackendState", default)]
        backend_state: String,
        #[serde(rename = "Self")]
        self_node: Option<Node>,
        #[serde(rename = "Peer", default, deserialize_with = "null_as_default")]
        peers: BTreeMap<String, Node>,
    }
    let raw: Raw = serde_json::from_slice(json)?;
    if raw.backend_state != "Running" {
        return Err(Error::NotRunning(raw.backend_state));
    }
    let self_node = raw
        .self_node
        .ok_or_else(|| Error::NotRunning("no self node".into()))?;
    Ok(Status {
        self_node,
        peers: raw.peers.into_values().collect(),
    })
}

pub fn parse_whois(json: &[u8]) -> Result<WhoIs, Error> {
    Ok(serde_json::from_slice(json)?)
}

const MACOS_APP_CLI: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";

/// The OS-installed Tailscale, driven through its CLI.
///
/// shortcut: shells out to `tailscale status/whois --json` (one process per call, CLI output isn't a stable API);
/// upgrade to LocalAPI over the tailscaled socket, or tailscale-rs once it hits 1.0, when whois runs per connection.
pub struct TailscaleCli {
    binaries: Vec<PathBuf>,
}

impl Default for TailscaleCli {
    fn default() -> Self {
        let mut binaries = vec![PathBuf::from("tailscale")];
        if cfg!(target_os = "macos") {
            // The Mac App Store / standalone app doesn't put its CLI on PATH.
            binaries.push(PathBuf::from(MACOS_APP_CLI));
        }
        Self { binaries }
    }
}

impl TailscaleCli {
    /// Tries each binary in order, skipping ones that don't exist.
    pub fn with_binaries(binaries: Vec<PathBuf>) -> Self {
        Self { binaries }
    }

    fn run(&self, args: &[&str]) -> Result<std::process::Output, Error> {
        for bin in &self.binaries {
            match Command::new(bin).args(args).output() {
                Ok(out) => return Ok(out),
                Err(e) if e.kind() == ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Err(Error::NotInstalled)
    }

    pub fn status(&self) -> Result<Status, Error> {
        let out = self.run(&["status", "--json"])?;
        // A stopped backend still prints JSON; an unreachable daemon prints only an error.
        match parse_status(&out.stdout) {
            Err(Error::Parse(_)) if !out.status.success() => Err(Error::NotRunning(
                String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            )),
            result => result,
        }
    }
}

impl Tailnet for TailscaleCli {
    fn self_node(&self) -> Result<Node, Error> {
        Ok(self.status()?.self_node)
    }

    fn peers(&self) -> Result<Vec<Node>, Error> {
        Ok(self.status()?.peers)
    }

    fn whois(&self, addr: SocketAddr) -> Result<WhoIs, Error> {
        let out = self.run(&["whois", "--json", &addr.to_string()])?;
        if !out.status.success() {
            return Err(Error::Command {
                command: "whois",
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            });
        }
        parse_whois(&out.stdout)
    }
}

/// Go marshals empty slices and maps as `null`.
fn null_as_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}
