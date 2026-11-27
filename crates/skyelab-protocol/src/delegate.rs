//! Delegation across machines (1.8): an agent on the calling machine lists projects on, starts threads on, and
//! drives threads it started on the serving machine. The serving machine takes the caller's node and device key
//! from the connection, never from these messages, and decides the account from its own folder binding.

use serde::{Deserialize, Serialize};

/// The minor version that added `delegate`.
pub const DELEGATION_SINCE_MINOR: u32 = 8;

/// The thread on the calling machine whose agent makes the call, as that machine says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delegator {
    /// The calling machine's store id: thread ids restart in a fresh store, so children are scoped by it.
    pub store_id: String,
    pub thread_id: i64,
    pub title: String,
    /// The caller's account id on the calling machine (a local name there).
    pub account_id: String,
    /// That account's label there, for the user's eyes only; an older 1.8 peer sends none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,
    /// That account's login, as the calling machine's CLI reports it.
    pub login: Login,
    /// How many delegations deep the caller is (0: a user's own thread).
    pub depth: u32,
}

/// A provider login: `provider` is `"claude"`; only Claude logins carry an org id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Login {
    pub provider: String,
    pub email: String,
    pub org_id: String,
}

/// What the caller asks. Unknown ops are a bad request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DelegateOp {
    /// Projects whose folder binds to the account the caller is linked to here.
    ListProjects,
    /// Starts a thread. A repeat with the same `request_id` from the same caller returns the same thread.
    Start {
        project_id: i64,
        prompt: String,
        #[serde(default)]
        worktree: bool,
        request_id: String,
    },
    Status {
        thread_id: i64,
    },
    Read {
        thread_id: i64,
        #[serde(default)]
        since_seq: i64,
    },
    Send {
        thread_id: i64,
        text: String,
    },
    /// One bounded wait round; the caller repeats rounds until its own deadline.
    Wait {
        thread_id: i64,
        timeout_s: u64,
    },
    /// Stops the child's turn (the caller couldn't record it, so it must not run unseen).
    Interrupt {
        thread_id: i64,
    },
}

/// Where a thread started by delegation came from (1.8), for its label on any machine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegatedFrom {
    /// The calling machine's MagicDNS name.
    pub machine: String,
    pub parent_thread_id: i64,
    pub parent_title: String,
}
