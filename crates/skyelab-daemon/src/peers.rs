//! Connections that passed the gate, so the user can see and kick them.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::sync::{Notify, watch};

/// A connection that passed the gate, as whois named it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    /// Unique for this daemon's life; pass it to [`Daemon::disconnect`].
    pub id: u64,
    pub addr: SocketAddr,
    /// MagicDNS name, without the trailing dot.
    pub device: String,
    pub user: String,
    /// The node's Tailscale StableID.
    pub node_id: String,
}

pub(crate) struct Entry {
    pub(crate) peer: Peer,
    pub(crate) kick: Arc<Notify>,
    /// Cancelled with every kick and when the connection ends, so calls already read from it don't act.
    pub(crate) gate: crate::CallGate,
    /// Set once hello passed with an approved key; a revoke of it closes the connection.
    pub(crate) approval: Option<crate::Approval>,
    /// Ports this connection forwards (1.5). Each tunnel watches its port's sender and ends when it drops.
    pub(crate) forwards: BTreeMap<u16, watch::Sender<()>>,
    /// A tunnel of another connection's forward: not listed as a connected device.
    pub(crate) tunnel: bool,
}

pub(crate) type Peers = Arc<Mutex<BTreeMap<u64, Entry>>>;

/// Nothing that holds this lock can panic, but a poisoned map is still a usable map.
pub(crate) fn lock_peers(peers: &Peers) -> std::sync::MutexGuard<'_, BTreeMap<u64, Entry>> {
    peers
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Removes its peer from the registry when the connection ends, however it ends, and cancels the calls it made:
/// nobody is waiting for them, and once the entry is gone a revoke could no longer reach them.
pub(crate) struct Registered<'a> {
    pub(crate) peers: &'a Peers,
    pub(crate) id: u64,
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        if let Some(entry) = lock_peers(self.peers).remove(&self.id) {
            entry.gate.cancel();
        }
    }
}
