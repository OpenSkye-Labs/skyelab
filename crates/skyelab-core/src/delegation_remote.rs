//! When an agent on another of the user's machines may start or drive a thread here. The folder's binding on this
//! machine decides the account, never the caller's. A start passes only when the user approved a link from the
//! caller's (device, account) to that account here, and both accounts report the same Claude login right now.

use crate::{Account, AccountsConfig, Provider};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Busy delegated threads one origin machine may have here across all its parents.
pub const MAX_PER_ORIGIN: usize = 8;
/// Longest prompt or message text a delegated call may carry, in chars.
pub const MAX_TEXT: usize = 100_000;
/// Longest parent title, in chars, after [`clean_title`].
pub const MAX_TITLE: usize = 200;
/// Longest account or request id, in bytes.
pub const MAX_ID: usize = 128;
pub const MAX_EMAIL: usize = 320;
pub const MAX_ORG: usize = 64;
/// Longest one wait round on this machine; the caller repeats rounds until its own deadline.
pub const MAX_WAIT_S: u64 = 20;

/// A provider login as its own CLI reports it. Only a Claude login with an org id can prove an identity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Login {
    pub provider: Provider,
    pub email: String,
    pub org_id: String,
}

impl Login {
    /// Whether this login can vouch for an account across machines.
    pub fn is_provable(&self) -> bool {
        self.provider == Provider::Claude && !self.email.is_empty() && !self.org_id.is_empty()
    }
}

/// The user's yes on this machine: agents on `node_id` (with that device key) under `origin_account` there may
/// start threads in folders bound to `local_account` here, while both report `login`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DelegationLink {
    /// The origin's Tailscale StableID, from whois when it was approved.
    pub node_id: String,
    /// SHA-256 of the origin's device key, hex.
    pub key_hash: String,
    /// The origin's MagicDNS name then, for the list.
    pub machine: String,
    pub origin_account: String,
    pub login: Login,
    pub local_account: String,
}

/// Why a delegated call was refused: no permission (`DelegationNotAllowed` on the wire), or a bad request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NotAllowed(String),
    BadRequest(String),
}

impl Refusal {
    pub fn message(&self) -> &str {
        match self {
            Refusal::NotAllowed(m) | Refusal::BadRequest(m) => m,
        }
    }
}

/// The link the user approved for this caller, if any.
pub fn link_for<'a>(
    links: &'a [DelegationLink],
    node_id: &str,
    key_hash: &str,
    origin_account: &str,
) -> Option<&'a DelegationLink> {
    links.iter().find(|l| {
        l.node_id == node_id && l.key_hash == key_hash && l.origin_account == origin_account
    })
}

/// The account a thread delegated through `link` runs under in `dir`: the folder's own binding, and only when that
/// is the link's account and a Claude one. No message names a path.
pub fn check_remote_target<'a>(
    config: &'a AccountsConfig,
    link: &DelegationLink,
    dir: &Path,
) -> Result<&'a Account, Refusal> {
    let Some(target) = config.resolve(dir) else {
        return Err(Refusal::BadRequest(
            "that project's folder isn't bound to an account on this machine; SkyeLab never picks one".into(),
        ));
    };
    if target.id != link.local_account {
        return Err(Refusal::NotAllowed(
            "that project runs under another account on this machine than the one this caller is linked to".into(),
        ));
    }
    if target.provider != Provider::Claude {
        return Err(Refusal::NotAllowed(format!(
            "{} accounts can't be delegated to across machines",
            target.provider.label()
        )));
    }
    Ok(target)
}

/// Both machines' logins now, against the one the user approved. Anything unread or unprovable refuses.
pub fn check_logins(
    link: &DelegationLink,
    caller: Option<&Login>,
    local: Option<&Login>,
) -> Result<(), Refusal> {
    let not = |why: &str| Err(Refusal::NotAllowed(why.into()));
    let Some(caller) = caller.filter(|l| l.is_provable()) else {
        return not(
            "the calling account's Claude login (email and organization) couldn't be confirmed",
        );
    };
    let Some(local) = local.filter(|l| l.is_provable()) else {
        return not("this machine's account login (email and organization) couldn't be read");
    };
    if caller != local || *caller != link.login {
        return not("the two machines' accounts aren't signed in to the same Claude login");
    }
    Ok(())
}

/// The depth a child of a caller at `caller_depth` gets here, or why it can't start.
pub fn remote_child_depth(caller_depth: u32) -> Result<u32, String> {
    crate::delegation::check_depth(caller_depth)?;
    Ok(caller_depth + 1)
}

/// Refuses one more busy delegated thread from an origin machine that already has `busy` here.
pub fn check_origin(busy: usize) -> Result<(), String> {
    if busy >= MAX_PER_ORIGIN {
        return Err(format!(
            "that machine already has {busy} delegated threads working here (max {MAX_PER_ORIGIN}); wait for one to finish"
        ));
    }
    Ok(())
}

/// Text from another machine, safe to store and draw: no control, bidi, zero-width or line-separator characters, at most
/// [`MAX_TITLE`] chars.
pub fn clean_title(title: &str) -> String {
    let bidi = |c: char| matches!(c, '\u{061C}' | '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}');
    title
        .chars()
        .filter(|&c| !c.is_control() && !bidi(c))
        .take(MAX_TITLE)
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
#[path = "delegation_remote_test.rs"]
mod tests;
