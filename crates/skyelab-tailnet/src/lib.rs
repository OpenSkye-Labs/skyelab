//! The user's own devices on their tailnet, and whether an incoming peer is one of them.
//!
//! No GPUI here: the settings UI calls [`Tailnet`] and matches on [`Error`].

use skyelab_core::process::Captured;
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde::{Deserialize, Deserializer};

mod own;
pub use own::{is_own_device, own_online_peers, verify_own_peer};

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
    /// Names `tailscale cert` can issue for. Empty when HTTPS certificates are off for the tailnet.
    pub cert_domains: Vec<String>,
}

/// This node's TLS certificate chain and private key, PEM-encoded, as `tailscale cert` wrote them.
#[derive(Clone, PartialEq, Eq)]
pub struct Cert {
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
}

impl std::fmt::Debug for Cert {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the key.
        f.debug_struct("Cert")
            .field("cert_pem", &String::from_utf8_lossy(&self.cert_pem))
            .finish_non_exhaustive()
    }
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
    #[error(
        "HTTPS certificates are off for this tailnet, so SkyeLab can't encrypt connections. Turn on MagicDNS and \
         HTTPS Certificates in the Tailscale admin console ({ADMIN_DNS_URL})"
    )]
    CertsDisabled,
    #[error("could not parse Tailscale's output")]
    Parse(#[from] serde_json::Error),
    #[error("could not run tailscale")]
    Io(#[from] std::io::Error),
}

/// Where MagicDNS and HTTPS Certificates are turned on, the fix for every "no certificate" error.
pub const ADMIN_DNS_URL: &str = "https://login.tailscale.com/admin/dns";

pub trait Tailnet {
    fn self_node(&self) -> Result<Node, Error>;
    fn peers(&self) -> Result<Vec<Node>, Error>;
    /// `addr` must be the accepted socket's `peer_addr()`, never a value the client supplied.
    fn whois(&self, addr: SocketAddr) -> Result<WhoIs, Error>;
    /// This node's TLS certificate for its MagicDNS `name`. Fails closed unless the backend can issue one.
    fn cert(&self, name: &str) -> Result<Cert, Error> {
        Err(Error::Command {
            command: "cert",
            stderr: format!("this tailnet backend can't issue a certificate for {name}"),
        })
    }
}

impl Node {
    /// The MagicDNS name without its trailing dot, or the hostname when MagicDNS is off.
    pub fn magic_dns_name(&self) -> &str {
        match self.dns_name.trim_end_matches('.') {
            "" => &self.hostname,
            dns => dns,
        }
    }
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
        #[serde(rename = "CertDomains", default, deserialize_with = "null_as_default")]
        cert_domains: Vec<String>,
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
        cert_domains: raw.cert_domains,
    })
}

pub fn parse_whois(json: &[u8]) -> Result<WhoIs, Error> {
    Ok(serde_json::from_slice(json)?)
}

const MACOS_APP_CLI: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";

/// The OS-installed Tailscale, driven through its CLI.
///
/// shortcut: shells out to `tailscale status/whois --json` and `tailscale cert` (one process per call, CLI output isn't a stable API);
/// upgrade to LocalAPI over the tailscaled socket, or tailscale-rs once it hits 1.0, when whois runs per connection.
pub struct TailscaleCli {
    binaries: Vec<PathBuf>,
    timeout: Duration,
    /// Where `cert` writes; `None` means certs can't be fetched.
    cert_dir: Option<PathBuf>,
    cert_timeout: Duration,
}

/// Long enough for a busy tailscaled, short enough that a hung one doesn't stall a connection gate.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
/// A first issuance goes through Let's Encrypt's DNS challenge and can take tens of seconds.
const DEFAULT_CERT_TIMEOUT: Duration = Duration::from_secs(90);
/// `tailscale cert` renews when the cached cert has less left than this, so a daily check always stays ahead.
const MIN_VALIDITY: &str = "336h";

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
            cert_dir: None,
            cert_timeout: DEFAULT_CERT_TIMEOUT,
        }
    }

    /// The directory `cert` writes into (on unix, made mode 0700).
    pub fn with_cert_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cert_dir = Some(dir.into());
        self
    }

    /// Bounds one `cert` call, status check included.
    pub fn with_cert_timeout(mut self, timeout: Duration) -> Self {
        self.cert_timeout = timeout;
        self
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
            let mut cmd = Command::new(bin);
            // A bare `tailscale` is looked up in this PATH (the login shell's, for a Dock-launched app).
            cmd.arg(command)
                .args(args)
                .env_clear()
                .envs(skyelab_core::env::inherited_agent_env(false))
                // Tailscale.app's binary acts as the CLI only with a TERM, which a Dock-launched app lacks.
                .env("TAILSCALE_BE_CLI", "1");
            let spawned = Captured::spawn(&mut cmd, None);
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
        mut run: Captured,
        command: &'static str,
        deadline: Instant,
    ) -> Result<Output, Error> {
        let timeout = Error::Timeout {
            command,
            after: self.timeout,
        };
        let status = loop {
            let polled = run.child.try_wait();
            if let Ok(Some(status)) = polled {
                break status;
            }
            if polled.is_err() || Instant::now() >= deadline {
                // Best effort: it may have exited in between, and either way we fail closed.
                let _ = run.child.kill();
                let _ = run.child.wait();
                return Err(polled.err().map_or(timeout, Error::Io));
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        Ok(run.output(status)?)
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

    fn cert(&self, name: &str) -> Result<Cert, Error> {
        // Status and cert share the cert budget, so a timeout must report that budget, not the CLI default.
        self.fetch_cert(name).map_err(|e| match e {
            Error::Timeout { command, .. } => Error::Timeout {
                command,
                after: self.cert_timeout,
            },
            e => e,
        })
    }
}

impl TailscaleCli {
    fn fetch_cert(&self, name: &str) -> Result<Cert, Error> {
        let refuse = |why: String| Error::Command {
            command: "cert",
            stderr: why,
        };
        // The name becomes a CLI argument and a file name: a MagicDNS name only, never a flag or a path.
        let valid = !name.is_empty()
            && !name.starts_with(['-', '.'])
            && !name.contains("..")
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.');
        if !valid {
            return Err(refuse(format!("{name:?} is not a MagicDNS name")));
        }
        let dir = self
            .cert_dir
            .as_deref()
            .ok_or_else(|| refuse("no directory to keep the certificate in".into()))?;
        let deadline = Instant::now() + self.cert_timeout;
        let domains = self.status_by(deadline)?.cert_domains;
        if domains.is_empty() {
            return Err(Error::CertsDisabled);
        }
        if !domains.iter().any(|d| d.eq_ignore_ascii_case(name)) {
            return Err(refuse(format!(
                "this node can only get certificates for {}, not {name}",
                domains.join(", ")
            )));
        }
        private_dir(dir)?;
        let (cert_file, key_file) = (
            dir.join(format!("{name}.crt")),
            dir.join(format!("{name}.key")),
        );
        let (cert_arg, key_arg) = (cert_file.to_string_lossy(), key_file.to_string_lossy());
        let out = self.run(
            "cert",
            &[
                "--min-validity",
                MIN_VALIDITY,
                "--cert-file",
                &cert_arg,
                "--key-file",
                &key_arg,
                name,
            ],
            deadline,
        )?;
        if !out.status.success() {
            return Err(refuse(
                String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            ));
        }
        Ok(Cert {
            cert_pem: std::fs::read(&cert_file)?,
            key_pem: std::fs::read(&key_file)?,
        })
    }
}

/// Creates `dir` if needed and, on unix, makes it owner-only: it holds this node's private key.
fn private_dir(dir: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Go marshals empty slices and maps as `null`.
fn null_as_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
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

#[cfg(test)]
mod own_peers_test;
