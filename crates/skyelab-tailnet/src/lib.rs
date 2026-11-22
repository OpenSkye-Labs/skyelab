//! The user's own devices on their tailnet, and whether an incoming peer is one of them.
//!
//! No GPUI here: the settings UI calls [`Tailnet`] and matches on [`Error`].

use std::collections::BTreeMap;
use std::io::{ErrorKind, Read};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

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
    /// The CLI didn't finish in time (a hung tailscaled).
    #[error("`tailscale {command}` did not answer within {after:?}")]
    Timeout {
        command: &'static str,
        after: Duration,
    },
    #[error("could not parse Tailscale's output")]
    Parse(#[from] serde_json::Error),
    #[error("could not run tailscale")]
    Io(#[from] std::io::Error),
}

pub trait Tailnet {
    fn self_node(&self) -> Result<Node, Error>;
    fn peers(&self) -> Result<Vec<Node>, Error>;
    /// `addr` must be the accepted socket's `peer_addr()`, never a value the client supplied.
    fn whois(&self, addr: SocketAddr) -> Result<WhoIs, Error>;
}

/// The security core: accept `whois` only if it is one of the user's own, untagged devices.
///
/// Tagged nodes belong to the tailnet, not a person, so they are rejected even when the same user tagged them.
/// A tagged self has no personal owner, and this node itself is rejected (another OS user here could reach it).
pub fn is_own_device(self_node: &Node, whois: &WhoIs) -> bool {
    let Some(me) = self_node.user_id.filter(|&id| id != 0) else {
        return false;
    };
    self_node.tags.is_empty()
        // An empty id can't prove the peer is a different node from this one.
        && !whois.node.stable_id.is_empty()
        && whois.node.stable_id != self_node.stable_id
        && whois.user_profile.id == Some(me)
        && whois.node.user == Some(me)
        && whois.node.tags.is_empty()
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
    timeout: Duration,
}

/// Long enough for a busy tailscaled, short enough that a hung one doesn't stall a connection gate.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

impl Default for TailscaleCli {
    fn default() -> Self {
        let mut binaries = Vec::new();
        if cfg!(target_os = "macos") {
            // The app doesn't put its CLI on PATH. Tried first: an absolute path can't be shadowed by PATH.
            binaries.push(PathBuf::from(MACOS_APP_CLI));
        }
        binaries.push(PathBuf::from("tailscale"));
        Self::with_binaries(binaries)
    }
}

impl TailscaleCli {
    /// Tries each binary in order, skipping ones that don't exist.
    pub fn with_binaries(binaries: Vec<PathBuf>) -> Self {
        Self {
            binaries,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Bounds every CLI call; one that runs longer is killed and returns [`Error::Timeout`].
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Runs one CLI call that must finish by `deadline`.
    fn run(
        &self,
        command: &'static str,
        args: &[&str],
        deadline: Instant,
    ) -> Result<Output, Error> {
        for bin in &self.binaries {
            let spawned = Command::new(bin)
                .arg(command)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
            match spawned {
                Ok(child) => return self.wait(child, command, deadline),
                Err(e) if e.kind() == ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Err(Error::NotInstalled)
    }

    /// A hung tailscaled must fail closed, not hang the caller.
    fn wait(
        &self,
        mut child: Child,
        command: &'static str,
        deadline: Instant,
    ) -> Result<Output, Error> {
        let timeout = Error::Timeout {
            command,
            after: self.timeout,
        };
        // Drain both pipes on their own threads, or a chatty child blocks on a full pipe and looks hung.
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        let status = loop {
            let polled = child.try_wait();
            if let Ok(Some(status)) = polled {
                break status;
            }
            if polled.is_err() || Instant::now() >= deadline {
                // Best effort: it may have exited in between, and either way we fail closed.
                let _ = child.kill();
                let _ = child.wait();
                return Err(polled.err().map_or(timeout, Error::Io));
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        // A grandchild that inherited the pipes could hold them open past the exit; still bounded by the deadline.
        let collect = |rx: mpsc::Receiver<Vec<u8>>| {
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        };
        match (collect(stdout), collect(stderr)) {
            (Ok(stdout), Ok(stderr)) => Ok(Output {
                status,
                stdout,
                stderr,
            }),
            _ => Err(timeout),
        }
    }

    pub fn status(&self) -> Result<Status, Error> {
        self.status_by(Instant::now() + self.timeout)
    }

    fn status_by(&self, deadline: Instant) -> Result<Status, Error> {
        let out = self.run("status", &["--json"], deadline)?;
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
        // One deadline for whois and its status follow-up, so the whole call stays within the timeout.
        let deadline = Instant::now() + self.timeout;
        let out = self.run("whois", &["--json", &addr.to_string()], deadline)?;
        if !out.status.success() {
            // Tell "Tailscale is down" (NotRunning) apart from "unknown peer".
            self.status_by(deadline)?;
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

fn drain(pipe: Option<impl Read + Send + 'static>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    if let Some(mut pipe) = pipe {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            // A read error just ends the output early; the exit status still tells the caller what happened.
            let _ = pipe.read_to_end(&mut buf);
            let _ = tx.send(buf);
        });
    } else {
        let _ = tx.send(Vec::new());
    }
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_cli_is_tried_before_path() {
        let want: &[&str] = if cfg!(target_os = "macos") {
            &[MACOS_APP_CLI, "tailscale"]
        } else {
            &["tailscale"]
        };
        let want: Vec<PathBuf> = want.iter().map(PathBuf::from).collect();
        assert_eq!(TailscaleCli::default().binaries, want);
    }

    #[cfg(unix)]
    #[test]
    fn app_cli_wins_when_both_exist() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("test setup");
        let fake = |name: &str| {
            let bin = dir.path().join(name);
            let marker = dir.path().join(format!("{name}.ran"));
            std::fs::write(&bin, format!("#!/bin/sh\ntouch '{}'\n", marker.display()))
                .expect("test setup");
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
                .expect("test setup");
            (bin, marker)
        };
        let (app, app_ran) = fake("app-cli");
        let (path, path_ran) = fake("path-cli");
        assert!(
            TailscaleCli::with_binaries(vec![app, path])
                .status()
                .is_err()
        );
        assert!(app_ran.exists() && !path_ran.exists());
    }
}
