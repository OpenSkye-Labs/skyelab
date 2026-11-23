//! Which device keys may use this machine: the user's approvals, the requests waiting for an answer, and the
//! keys turned down since the daemon started. Checked at hello, under one lock that a revoke also takes, so a
//! revoke can't miss a hello passing at the same moment.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use skyelab_protocol::{device_code, device_key_hash, is_device_key};

use crate::peers::{Peers, lock_peers};

/// One approved device: a client install's key, on the tailnet node it was approved for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    /// The node's Tailscale StableID, from whois.
    pub node_id: String,
    /// [`skyelab_protocol::device_key_hash`] of the client's key.
    pub key_hash: String,
}

/// A device that said hello with a key nobody approved yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessRequest {
    pub node_id: String,
    /// MagicDNS name, without the trailing dot.
    pub device: String,
    pub user: String,
    pub key_hash: String,
    /// What the device shows too, so the user can tell their own request from another.
    pub code: String,
}

/// Requests kept at once. A process that keeps making keys can't grow this; past the cap new ones are dropped.
pub(crate) const MAX_REQUESTS: usize = 16;
/// Requests kept per node; a newer one replaces the oldest.
const MAX_REQUESTS_PER_NODE: usize = 2;
/// Denials kept for the daemon's life; the oldest goes first.
const MAX_DENIED: usize = 64;
/// A request nobody answered is dropped after this long without a new try.
const REQUEST_TTL: Duration = Duration::from_secs(600);

#[derive(Default)]
pub(crate) struct Access {
    approved: Vec<Approval>,
    requests: Vec<(AccessRequest, Instant)>,
    denied: Vec<Approval>,
}

pub(crate) type SharedAccess = Arc<Mutex<Access>>;

pub(crate) fn lock(access: &SharedAccess) -> MutexGuard<'_, Access> {
    access.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Who whois said is on the other end.
pub(crate) struct Who {
    pub(crate) peer_id: u64,
    pub(crate) node_id: String,
    pub(crate) device: String,
    pub(crate) user: String,
}

pub(crate) enum Verdict {
    Approved,
    NotApproved,
    Denied,
}

impl Access {
    pub(crate) fn new(approved: Vec<Approval>) -> Self {
        Self {
            approved,
            ..Self::default()
        }
    }

    fn expire(&mut self) {
        self.requests
            .retain(|(_, seen)| seen.elapsed() < REQUEST_TTL);
    }

    pub(crate) fn requests(&mut self) -> Vec<AccessRequest> {
        self.expire();
        self.requests.iter().map(|(r, _)| r.clone()).collect()
    }
}

/// Hello's check. An approved key is recorded on the peer while the lock is held, so [`set_approved`] sees it.
pub(crate) fn check(access: &SharedAccess, peers: &Peers, who: &Who, key: Option<&str>) -> Verdict {
    let Some(key) = key.filter(|k| is_device_key(k)) else {
        return Verdict::NotApproved;
    };
    let wanted = Approval {
        node_id: who.node_id.clone(),
        key_hash: device_key_hash(key),
    };
    let mut access = lock(access);
    if access.denied.contains(&wanted) {
        return Verdict::Denied;
    }
    if access.approved.contains(&wanted) {
        if let Some(entry) = lock_peers(peers).get_mut(&who.peer_id) {
            entry.approval = Some(wanted);
        }
        return Verdict::Approved;
    }
    access.expire();
    let now = Instant::now();
    // A node that keeps making keys only replaces its own requests, so it can't push other devices off the list.
    let known = access
        .requests
        .iter()
        .any(|(r, _)| r.node_id == wanted.node_id && r.key_hash == wanted.key_hash);
    let from_node: Vec<usize> = (0..access.requests.len())
        .filter(|&i| access.requests[i].0.node_id == wanted.node_id)
        .collect();
    if !known && from_node.len() >= MAX_REQUESTS_PER_NODE {
        access.requests.remove(from_node[0]);
    }
    let full = access.requests.len() >= MAX_REQUESTS;
    match access
        .requests
        .iter_mut()
        .find(|(r, _)| r.node_id == wanted.node_id && r.key_hash == wanted.key_hash)
    {
        Some((_, seen)) => *seen = now,
        None if full => eprintln!(
            "skyelab-daemon: not listing a request from {:?}: {MAX_REQUESTS} already wait",
            who.device
        ),
        None => access.requests.push((
            AccessRequest {
                code: device_code(&wanted.key_hash),
                node_id: wanted.node_id,
                device: who.device.clone(),
                user: who.user.clone(),
                key_hash: wanted.key_hash,
            },
            now,
        )),
    }
    Verdict::NotApproved
}

/// Replaces the approvals and closes every connection whose approval is no longer among them.
pub(crate) fn set_approved(access: &SharedAccess, peers: &Peers, approved: Vec<Approval>) {
    let mut access = lock(access);
    access.requests.retain(|(r, _)| {
        !approved
            .iter()
            .any(|a| a.node_id == r.node_id && a.key_hash == r.key_hash)
    });
    access.denied.retain(|d| !approved.contains(d));
    for entry in lock_peers(peers).values() {
        if entry
            .approval
            .as_ref()
            .is_some_and(|a| !approved.contains(a))
        {
            // notify_one keeps the wakeup if the serve loop isn't waiting right now.
            entry.kick.notify_one();
        }
    }
    access.approved = approved;
}

pub(crate) fn deny(access: &SharedAccess, node_id: &str, key_hash: &str) {
    let denied = Approval {
        node_id: node_id.to_owned(),
        key_hash: key_hash.to_owned(),
    };
    let mut access = lock(access);
    access
        .requests
        .retain(|(r, _)| !(r.node_id == node_id && r.key_hash == key_hash));
    if !access.denied.contains(&denied) {
        if access.denied.len() >= MAX_DENIED {
            access.denied.remove(0);
        }
        access.denied.push(denied);
    }
}
