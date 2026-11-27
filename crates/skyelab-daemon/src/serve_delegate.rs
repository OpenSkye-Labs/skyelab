//! `delegate` (1.8): an agent on another of the user's machines asks this one to start or drive a thread. The
//! daemon checks sizes before the backend sees the call, names the caller from the connection itself (whois and
//! the approved hello, never the message), and lists a refused caller for the user here, as hello lists devices.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;
use skyelab_core::delegation_remote::{
    MAX_EMAIL, MAX_ID, MAX_ORG, MAX_TEXT, MAX_TITLE, MAX_WAIT_S,
};
use skyelab_protocol::{DelegateOp, Delegator, ErrorCode, RpcError};

use crate::access::Who;
use crate::peers::lock_peers;
use crate::{Backend, Daemon};

/// The calling machine as this connection proved it: whois's node and name, and the approved device key's hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub node_id: String,
    /// MagicDNS name, without the trailing dot.
    pub device: String,
    pub key_hash: String,
}

/// A caller refused because this machine hasn't allowed it (no link, or one that doesn't match), for the user here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationRequest {
    pub origin: Origin,
    /// The caller's account and login there, as that machine says.
    pub account_id: String,
    pub email: String,
    pub org_id: String,
    pub provider: String,
}

/// Requests kept at once, and per node; a newer one from a full node replaces its oldest.
const MAX_REQUESTS: usize = 16;
const MAX_PER_NODE: usize = 2;
const REQUEST_TTL: Duration = Duration::from_secs(600);
const MAX_DENIED: usize = 64;

#[derive(Default)]
pub(crate) struct Requests {
    waiting: Vec<(DelegationRequest, Instant)>,
    /// (node, key hash, account) the user turned down since the daemon started.
    denied: Vec<(String, String, String)>,
}

pub(crate) type SharedRequests = Arc<Mutex<Requests>>;

fn lock(r: &SharedRequests) -> MutexGuard<'_, Requests> {
    r.lock().unwrap_or_else(PoisonError::into_inner)
}

fn key(r: &DelegationRequest) -> (&str, &str, &str) {
    (&r.origin.node_id, &r.origin.key_hash, &r.account_id)
}

impl Requests {
    fn expire(&mut self) {
        self.waiting
            .retain(|(_, seen)| seen.elapsed() < REQUEST_TTL);
    }

    fn is_denied(&self, r: &DelegationRequest) -> bool {
        self.denied
            .iter()
            .any(|(n, k, a)| (n.as_str(), k.as_str(), a.as_str()) == key(r))
    }

    fn note(&mut self, request: DelegationRequest) {
        self.expire();
        let now = Instant::now();
        if let Some((_, seen)) = self
            .waiting
            .iter_mut()
            .find(|(r, _)| key(r) == key(&request))
        {
            *seen = now;
            return;
        }
        let from_node: Vec<usize> = (0..self.waiting.len())
            .filter(|&i| self.waiting[i].0.origin.node_id == request.origin.node_id)
            .collect();
        if from_node.len() >= MAX_PER_NODE {
            self.waiting.remove(from_node[0]);
        }
        if self.waiting.len() < MAX_REQUESTS {
            self.waiting.push((request, now));
        }
    }
}

/// Bounds on every field, checked before anything else.
fn bounds(caller: &Delegator, op: &DelegateOp) -> Result<(), String> {
    let chars = |s: &str| s.chars().count();
    let checks = [
        (
            chars(&caller.title) <= MAX_TITLE,
            "the caller's title is too long",
        ),
        (
            caller.account_id.len() <= MAX_ID,
            "the account id is too long",
        ),
        (caller.store_id.len() <= MAX_ID, "the store id is too long"),
        (
            caller.login.email.len() <= MAX_EMAIL,
            "the email is too long",
        ),
        (
            caller.login.org_id.len() <= MAX_ORG,
            "the org id is too long",
        ),
        (
            caller.login.provider.len() <= MAX_ID,
            "the provider is too long",
        ),
    ];
    if let Some((_, why)) = checks.iter().find(|(ok, _)| !ok) {
        return Err((*why).to_owned());
    }
    match op {
        DelegateOp::Start {
            prompt, request_id, ..
        } => {
            if chars(prompt) > MAX_TEXT {
                return Err(format!("the prompt is longer than {MAX_TEXT} characters"));
            }
            if request_id.is_empty() || request_id.len() > MAX_ID {
                return Err("the request id must be 1 to 128 bytes".into());
            }
        }
        DelegateOp::Send { text, .. } if chars(text) > MAX_TEXT => {
            return Err(format!("the message is longer than {MAX_TEXT} characters"));
        }
        DelegateOp::Wait { timeout_s, .. } if *timeout_s > MAX_WAIT_S => {
            return Err(format!(
                "one wait is at most {MAX_WAIT_S} seconds; call again"
            ));
        }
        _ => {}
    }
    Ok(())
}

/// What a `delegate` call needs from its connection, taken before the call leaves the serve loop.
pub(crate) struct Ctx {
    origin: Option<Origin>,
    requests: SharedRequests,
}

impl Ctx {
    pub(crate) fn new(shared: &crate::Shared, who: &Who) -> Self {
        let origin = lock_peers(&shared.peers)
            .get(&who.peer_id)
            .and_then(|e| e.approval.clone())
            .filter(|a| a.node_id == who.node_id)
            .map(|a| Origin {
                node_id: who.node_id.clone(),
                device: who.device.trim_end_matches('.').to_owned(),
                key_hash: a.key_hash,
            });
        Self {
            origin,
            requests: shared.delegation.clone(),
        }
    }

    /// Bounds, the user's Deny, then the backend. A refusal for want of permission is listed for the user.
    pub(crate) fn run(
        &self,
        backend: &dyn Backend,
        caller: Delegator,
        op: DelegateOp,
    ) -> Result<Value, RpcError> {
        bounds(&caller, &op).map_err(|why| RpcError::new(ErrorCode::BadRequest, why))?;
        let origin = self.origin.clone().ok_or_else(|| {
            RpcError::new(ErrorCode::NotApproved, "this device isn't approved here")
        })?;
        let request = DelegationRequest {
            origin: origin.clone(),
            account_id: caller.account_id.clone(),
            email: caller.login.email.clone(),
            org_id: caller.login.org_id.clone(),
            provider: caller.login.provider.clone(),
        };
        let not_allowed = |why: &str| RpcError::new(ErrorCode::DelegationNotAllowed, why);
        if lock(&self.requests).is_denied(&request) {
            return Err(not_allowed(
                "delegation from that machine and account was turned down here",
            ));
        }
        let result = backend.delegate(&origin, caller, op);
        if let Err(e) = &result
            && e.code == ErrorCode::DelegationNotAllowed
        {
            let mut requests = lock(&self.requests);
            if !requests.is_denied(&request) {
                requests.note(request);
            }
        }
        result
    }
}

impl Daemon {
    /// Callers refused for want of a delegation link, oldest first.
    pub fn delegation_requests(&self) -> Vec<DelegationRequest> {
        let mut r = lock(&self.delegation);
        r.expire();
        r.waiting.iter().map(|(r, _)| r.clone()).collect()
    }

    /// The user allowed (or saw) this caller: its request goes.
    pub fn forget_delegation_request(&self, node_id: &str, key_hash: &str, account_id: &str) {
        lock(&self.delegation)
            .waiting
            .retain(|(r, _)| key(r) != (node_id, key_hash, account_id));
    }

    /// Turns this caller down until the daemon stops: its request goes, and its calls are refused unlisted.
    pub fn deny_delegation(&self, node_id: &str, key_hash: &str, account_id: &str) {
        self.forget_delegation_request(node_id, key_hash, account_id);
        let mut r = lock(&self.delegation);
        let denied = (
            node_id.to_owned(),
            key_hash.to_owned(),
            account_id.to_owned(),
        );
        if !r.denied.contains(&denied) {
            if r.denied.len() >= MAX_DENIED {
                r.denied.remove(0);
            }
            r.denied.push(denied);
        }
    }
}
