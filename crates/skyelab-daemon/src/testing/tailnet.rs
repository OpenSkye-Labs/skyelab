//! The fake tailnet the remote-test harness serves on: who whois says is calling, and this node's own view.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use skyelab_tailnet::{Cert, Error as TailnetError, Node, Tailnet, UserProfile, WhoIs, WhoIsNode};

use super::{ME, NODE_NAME, ca};

/// Who `FakeTailnet::whois` says is calling.
#[derive(Clone, Copy, Debug)]
pub enum Peer {
    /// One of the user's own untagged devices: the gate admits it.
    Own,
    /// Another of the user's own untagged devices, with this StableID.
    OwnNode(&'static str),
    OtherUser,
    /// The user's node, but tagged: refused.
    Tagged,
    WhoisError,
    /// Admitted, after this long.
    Hang(Duration),
}

/// A tailnet node on loopback.
pub fn node(id: &str, dns: &str, user: u64, online: bool, tags: &[&str]) -> Node {
    Node {
        stable_id: id.into(),
        hostname: dns.split('.').next().unwrap_or_default().into(),
        dns_name: format!("{dns}."),
        tailscale_ips: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
        os: "macOS".into(),
        online,
        user_id: Some(user),
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
    }
}

/// The daemon's view of the tailnet: this node is `NODE_NAME` on `ips` (127.0.0.1), `peers` is what
/// `tailscale status` lists, and whois answers as `peer` says. The atomic and `Mutex` knobs can change while a
/// daemon uses them.
pub struct FakeTailnet {
    pub ips: Vec<IpAddr>,
    /// `self_node` fails like a stopped Tailscale.
    pub down: AtomicBool,
    pub peer: Mutex<Peer>,
    /// Overrides `peer` for callers from these addresses.
    pub peer_by_ip: Mutex<HashMap<IpAddr, Peer>>,
    pub peers: Vec<Node>,
    pub whois_calls: AtomicUsize,
    /// What `cert` returns; an `Err` is the CLI's error text.
    pub cert: Mutex<Result<Cert, String>>,
    /// `cert` sleeps this long first (a hung `tailscale cert`).
    pub cert_hang: Mutex<Duration>,
    pub cert_calls: AtomicUsize,
    /// `Self.DNSName`; empty means MagicDNS is off.
    pub dns_name: Mutex<String>,
    /// This node's StableID; `None` is `nSelf`.
    pub self_id: Mutex<Option<String>>,
}

impl Default for FakeTailnet {
    fn default() -> Self {
        Self {
            ips: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
            down: AtomicBool::new(false),
            peer: Mutex::new(Peer::Own),
            peer_by_ip: Mutex::default(),
            peers: Vec::new(),
            whois_calls: AtomicUsize::new(0),
            cert: Mutex::new(Ok(ca().issue(NODE_NAME))),
            cert_hang: Mutex::new(Duration::ZERO),
            cert_calls: AtomicUsize::new(0),
            dns_name: Mutex::new(format!("{NODE_NAME}.")),
            self_id: Mutex::new(None),
        }
    }
}

impl FakeTailnet {
    pub fn new(peer: Peer) -> Arc<Self> {
        Arc::new(Self {
            peer: Mutex::new(peer),
            ..Self::default()
        })
    }

    pub fn set_peer(&self, peer: Peer) {
        *self.peer.lock().expect("peer lock") = peer;
    }

    pub fn whois_calls(&self) -> usize {
        self.whois_calls.load(Ordering::SeqCst)
    }
}

fn whois(user: u64, tags: Vec<String>) -> WhoIs {
    whois_node("nPhone", user, tags)
}

fn whois_node(stable_id: &str, user: u64, tags: Vec<String>) -> WhoIs {
    WhoIs {
        node: WhoIsNode {
            stable_id: stable_id.into(),
            name: "phone.example.ts.net.".into(),
            user: Some(user),
            tags,
        },
        user_profile: UserProfile {
            id: Some(user),
            login_name: format!("user{user}@example.com"),
            display_name: "Someone".into(),
        },
    }
}

impl Tailnet for FakeTailnet {
    fn self_node(&self) -> Result<Node, TailnetError> {
        if self.down.load(Ordering::SeqCst) {
            return Err(TailnetError::NotRunning("Stopped".into()));
        }
        let id = self.self_id.lock().expect("self id lock").clone();
        Ok(Node {
            tailscale_ips: self.ips.clone(),
            dns_name: self.dns_name.lock().expect("dns lock").clone(),
            ..node(id.as_deref().unwrap_or("nSelf"), NODE_NAME, ME, true, &[])
        })
    }

    fn peers(&self) -> Result<Vec<Node>, TailnetError> {
        Ok(self.peers.clone())
    }

    fn whois(&self, addr: SocketAddr) -> Result<WhoIs, TailnetError> {
        self.whois_calls.fetch_add(1, Ordering::SeqCst);
        // Copy out first: holding the lock through a Hang would serialize every whois.
        let by_ip = self
            .peer_by_ip
            .lock()
            .expect("peer lock")
            .get(&addr.ip())
            .copied();
        let peer = by_ip.unwrap_or_else(|| *self.peer.lock().expect("peer lock"));
        match peer {
            Peer::Own => Ok(whois(ME, vec![])),
            Peer::OwnNode(id) => Ok(whois_node(id, ME, vec![])),
            Peer::OtherUser => Ok(whois(2002, vec![])),
            Peer::Tagged => Ok(whois(ME, vec!["tag:server".into()])),
            Peer::WhoisError => Err(TailnetError::Command {
                command: "whois",
                stderr: "no such peer".into(),
            }),
            Peer::Hang(d) => {
                std::thread::sleep(d);
                Ok(whois(ME, vec![]))
            }
        }
    }

    fn cert(&self, _name: &str) -> Result<Cert, TailnetError> {
        self.cert_calls.fetch_add(1, Ordering::SeqCst);
        let hang = *self.cert_hang.lock().expect("cert lock");
        std::thread::sleep(hang);
        self.cert
            .lock()
            .expect("cert lock")
            .clone()
            .map_err(|stderr| TailnetError::Command {
                command: "cert",
                stderr,
            })
    }
}
