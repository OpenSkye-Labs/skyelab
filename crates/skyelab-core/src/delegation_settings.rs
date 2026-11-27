//! Delegation across machines in `settings.toml`: the links this machine allowed in, and the machines this one's
//! agents may delegate to (Settings → Machines, off by default).

use crate::AppSettings;
use crate::delegation_remote::DelegationLink;
use serde::{Deserialize, Serialize};

/// Links kept at once; the oldest goes first.
const MAX_LINKS: usize = 64;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DelegationSettings {
    /// Callers on other machines the user allowed to start threads here.
    pub links: Vec<DelegationLink>,
    /// Linked machines (MagicDNS names, lowercase) this machine's agents may delegate to.
    pub targets: Vec<String>,
}

impl AppSettings {
    /// Saves `link`, replacing one for the same (node, key, origin account).
    pub fn link_delegation(&mut self, link: DelegationLink) {
        let links = &mut self.delegation.links;
        links.retain(|l| {
            !(l.node_id == link.node_id
                && l.key_hash == link.key_hash
                && l.origin_account == link.origin_account)
        });
        if links.len() >= MAX_LINKS {
            links.remove(0);
        }
        links.push(link);
    }

    /// Removes the link for this caller. False if there was none.
    pub fn revoke_delegation(
        &mut self,
        node_id: &str,
        key_hash: &str,
        origin_account: &str,
    ) -> bool {
        let before = self.delegation.links.len();
        self.delegation.links.retain(|l| {
            !(l.node_id == node_id && l.key_hash == key_hash && l.origin_account == origin_account)
        });
        self.delegation.links.len() != before
    }

    /// Turns "Let my agents delegate here" on or off for `host`.
    pub fn set_delegation_target(&mut self, host: &str, on: bool) {
        let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
        if host.is_empty() {
            return;
        }
        self.delegation.targets.retain(|h| *h != host);
        if on {
            self.delegation.targets.push(host);
        }
    }

    pub fn is_delegation_target(&self, host: &str) -> bool {
        let host = host.trim_end_matches('.');
        self.delegation
            .targets
            .iter()
            .any(|h| h.eq_ignore_ascii_case(host))
    }
}
