//! Client ↔ environment RPC: JSON messages, one per WebSocket text frame.
//!
//! The client sends [`Request`]s, each with an id. The environment answers each with a [`ServerMessage::Response`]
//! or [`ServerMessage::Error`] carrying the same id, and pushes [`ServerMessage::Event`]s on its own.
//! The first request on a connection must be [`Call::Hello`]; [`negotiate`] decides whether the versions match.

use serde::{Deserialize, Serialize};

/// Minor bumps only add optional fields, methods or error codes. A major bump breaks the wire format.
pub const VERSION: Version = Version { major: 1, minor: 1 };

/// The TCP port an environment's daemon listens on, and a client dials, unless configured otherwise.
pub const DEFAULT_PORT: u16 = 7287;

pub type RequestId = u64;

/// True for a MagicDNS name, `<machine>.<tailnet>.ts.net` (any case, no trailing dot): the only names a person may
/// type for another machine. `tailscale cert` issues certificates only for these, and the client checks the
/// certificate against the exact name, so an IP, `localhost` or a short name could never connect anyway.
pub fn is_magicdns_name(name: &str) -> bool {
    let label = |l: &str| {
        (1..=63).contains(&l.len())
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    let labels: Vec<&str> = name.split('.').collect();
    matches!(labels.as_slice(), [machine, tailnet, ts, net]
        if label(machine) && label(tailnet) && ts.eq_ignore_ascii_case("ts") && net.eq_ignore_ascii_case("net"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

/// The version this environment speaks to `client`, or [`ErrorCode::UnsupportedVersion`] for another major.
pub fn negotiate(client: Version) -> Result<Version, RpcError> {
    if client.major == VERSION.major {
        Ok(VERSION)
    } else {
        Err(RpcError::new(
            ErrorCode::UnsupportedVersion,
            format!(
                "protocol {}.x is not supported, this environment speaks {}.{}",
                client.major, VERSION.major, VERSION.minor
            ),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: RequestId,
    #[serde(flatten)]
    pub call: Call,
}

impl Request {
    /// Parses one text frame. On failure, the error to send back, with the request's id when it had a readable one.
    pub fn parse(text: &str) -> Result<Self, ServerMessage> {
        serde_json::from_str(text).map_err(|e| {
            #[derive(Deserialize)]
            struct IdOnly {
                id: RequestId,
            }
            ServerMessage::Error {
                id: serde_json::from_str::<IdOnly>(text).ok().map(|r| r.id),
                error: RpcError::new(ErrorCode::BadRequest, e.to_string()),
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Call {
    Hello {
        protocol: Version,
        #[serde(default)]
        capabilities: Vec<String>,
    },
    ListProjects,
    ListThreads {
        project_id: i64,
    },
    /// Events with `seq` greater than `seq`, oldest first, at most a page of them: ask again from the last
    /// seq until the reply is empty. `seq: 0` starts at the beginning.
    ThreadEventsSince {
        thread_id: i64,
        seq: i64,
    },
    StartThread {
        project_id: i64,
        prompt: String,
    },
    SendMessage {
        thread_id: i64,
        text: String,
    },
    Interrupt {
        thread_id: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Response {
        id: RequestId,
        result: Reply,
    },
    /// `id` is `None` only when the request was unreadable.
    Error {
        id: Option<RequestId>,
        error: RpcError,
    },
    /// A new event on a thread, pushed to every connection that said hello.
    Event(ThreadEvent),
}

impl ServerMessage {
    pub fn to_json(&self) -> String {
        // Plain structs, strings and integers: serializing can't fail.
        serde_json::to_string(self)
            .unwrap_or_else(|_| unreachable!("ServerMessage always serializes"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Reply {
    Hello(Hello),
    Projects(Vec<Project>),
    Threads(Vec<Thread>),
    Events(Vec<ThreadEvent>),
    ThreadStarted {
        thread_id: i64,
    },
    /// For calls with nothing to return (`send_message`, `interrupt`).
    Ok,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: Version,
    pub capabilities: Vec<String>,
    /// The serving machine's Tailscale node StableID (1.1). A client pins it per name, so a name that moves to
    /// another node is noticed. `None` from a 1.0 server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: ErrorCode,
    pub message: String,
}

impl RpcError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    UnsupportedVersion,
    HelloRequired,
    BadRequest,
    NotFound,
    TooManyRequests,
    /// The method exists but this environment can't run it yet.
    NotWired,
    Internal,
    /// A code from a newer minor version.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    /// The account the thread runs under.
    pub account_id: String,
    /// That account's label in the serving machine's accounts.toml (1.1). `None` from a 1.0 server, or when the
    /// account isn't there any more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,
    pub status: String,
    /// Unix milliseconds.
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadEvent {
    pub thread_id: i64,
    /// 1, 2, 3… per thread. A gap means missed pushes: fetch them with `thread_events_since`.
    pub seq: i64,
    pub kind: String,
    /// The agent layer's JSON, passed through as text.
    pub payload_json: String,
    pub created_at: i64,
}
