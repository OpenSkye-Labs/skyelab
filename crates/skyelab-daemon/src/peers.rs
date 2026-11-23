//! Connections that passed the gate, so the user can see and kick them.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

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
    /// Cancelled with every kick, so calls already read from this connection don't act.
    pub(crate) gate: crate::CallGate,
    /// Set once hello passed with an approved key; a revoke of it closes the connection.
    pub(crate) approval: Option<crate::Approval>,
}

pub(crate) type Peers = Arc<Mutex<BTreeMap<u64, Entry>>>;

/// Nothing that holds this lock can panic, but a poisoned map is still a usable map.
pub(crate) fn lock_peers(peers: &Peers) -> std::sync::MutexGuard<'_, BTreeMap<u64, Entry>> {
    peers
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Removes its peer from the registry when the connection ends, however it ends.
pub(crate) struct Registered<'a> {
    pub(crate) peers: &'a Peers,
    pub(crate) id: u64,
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        lock_peers(self.peers).remove(&self.id);
    }
}
