//! Which nodes are the user's own devices: the ownership rule every trust check shares.

use std::net::SocketAddr;

use crate::{Node, Tailnet, WhoIs};

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

/// The client side of the same rule: asks THIS machine's Tailscale who `peer` (an address just dialled) is, and
/// returns its node StableID only if it is one of the user's own, untagged devices. Any error fails closed.
pub fn verify_own_peer(tailnet: &dyn Tailnet, peer: SocketAddr) -> Result<String, String> {
    let me = tailnet.self_node().map_err(|e| e.to_string())?;
    let who = tailnet.whois(peer).map_err(|e| e.to_string())?;
    if !is_own_device(&me, &who) {
        return Err(format!(
            "{peer} is node {:?} of user {:?}{}, not one of your own untagged devices",
            who.node.stable_id,
            who.user_profile.login_name,
            if who.node.tags.is_empty() {
                String::new()
            } else {
                format!(" tagged {}", who.node.tags.join(", "))
            }
        ));
    }
    Ok(who.node.stable_id)
}

/// The peers an environment picker offers: the user's own untagged devices that are online, never this node.
/// The same ownership rule as [`is_own_device`], from `status` instead of `whois`.
pub fn own_online_peers(self_node: &Node, peers: Vec<Node>) -> Vec<Node> {
    let Some(me) = self_node.user_id.filter(|&id| id != 0) else {
        return Vec::new();
    };
    if !self_node.tags.is_empty() {
        return Vec::new();
    }
    peers
        .into_iter()
        .filter(|p| {
            p.online
                && p.tags.is_empty()
                && p.user_id == Some(me)
                && !p.stable_id.is_empty()
                && p.stable_id != self_node.stable_id
        })
        .collect()
}
