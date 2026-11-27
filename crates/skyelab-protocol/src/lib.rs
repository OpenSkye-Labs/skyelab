//! Client ↔ environment RPC: JSON messages, one per WebSocket text frame.
//!
//! The client sends [`Request`]s, each with an id. The environment answers each with a [`ServerMessage::Response`]
//! or [`ServerMessage::Error`] carrying the same id, and pushes [`ServerMessage::Event`]s on its own.
//! The first request on a connection must be [`Call::Hello`]; [`negotiate`] decides whether the versions match.

use serde::{Deserialize, Serialize};

mod delegate;
mod shell;
pub use delegate::{DELEGATION_SINCE_MINOR, DelegateOp, DelegatedFrom, Delegator, Login};
pub use shell::{ShellChange, ShellDelta, ShellSnapshot};

/// Minor bumps only add optional fields, methods or error codes. A major bump breaks the wire format.
pub const VERSION: Version = Version { major: 1, minor: 8 };

/// The minor version that added port forwarding (`forward_port`, `stop_forward`, `open_tunnel`).
pub const FORWARDS_SINCE_MINOR: u32 = 5;

/// The minor version that added `shelve_thread` and the thread's settled and snoozed state.
pub const SHELVES_SINCE_MINOR: u32 = 6;

/// The minor version that added the shell stream (`subscribe_shell`, [`ServerMessage::Shell`]) and the thread row's
/// sidebar fields.
pub const SHELL_SINCE_MINOR: u32 = 7;

/// The hello capability a tunnel connection declares (1.5): it may only call `open_tunnel`, gets no pushed events,
/// and isn't listed as a connected device.
pub const TUNNEL_CAPABILITY: &str = "tunnel";

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

/// A new random device key: 32 bytes as 64 lowercase hex chars. A client makes one per install and keeps it; the
/// machine it connects to approves the key, not just the device.
pub fn new_device_key() -> std::io::Result<String> {
    use ring::rand::SecureRandom as _;
    let mut bytes = [0u8; 32];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| std::io::Error::other("the system random number generator failed"))?;
    Ok(hex(&bytes))
}

/// 64 lowercase hex chars, the only form [`new_device_key`] makes.
pub fn is_device_key(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// What the serving machine stores and compares: SHA-256 of the key, hex. The key itself never leaves the client
/// except in hello, over TLS.
pub fn device_key_hash(key: &str) -> String {
    hex(ring::digest::digest(&ring::digest::SHA256, key.as_bytes()).as_ref())
}

/// The code both screens show for a key, four groups of four hex digits, so the person approving can tell their
/// device's request from another one. 64 bits of the hash: too many to grind a look-alike key for a code seen on screen.
pub fn device_code(key_hash: &str) -> String {
    let head = key_hash.get(..16).unwrap_or(key_hash).to_ascii_uppercase();
    head.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join("-")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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
        /// This client install's key (1.2). The serving machine admits only keys the user approved there, for the
        /// device they were approved on.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_key: Option<String>,
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
    /// Answers a permission request the thread's agent is waiting on (1.3): `request_id` is the `id` of the
    /// agent event `{"Permission": {"id", "tool", "input"}}`.
    AnswerPermission {
        thread_id: i64,
        request_id: String,
        decision: PermissionAnswer,
    },
    /// Answers an AskUserQuestion the thread's agent is waiting on (1.4): one list of picked labels (or typed text)
    /// per question, in the question order of the agent event `{"Permission": {"id", "tool": "AskUserQuestion"}}`.
    AnswerQuestions {
        thread_id: i64,
        request_id: String,
        answers: Vec<Vec<String>>,
    },
    /// Lets this connection's device tunnel to `port` on the serving machine's loopback (1.5), until it calls
    /// `stop_forward` or this connection ends. Answered with [`Reply::Forwarded`]. Refused for system, own and
    /// denylisted ports, and when nothing listens there.
    ForwardPort {
        port: u16,
    },
    /// Ends the forward `token` names and every tunnel opened through it (1.5).
    StopForward {
        port: u16,
        token: String,
    },
    /// Turns this connection, which declared [`TUNNEL_CAPABILITY`] in hello, into a tunnel to `port` on the serving
    /// machine's loopback (1.5). Allowed only while another live connection of the same approved device holds the
    /// forward `token` names. After the `Ok` reply the connection carries raw TCP bytes in binary frames both ways,
    /// and nothing else.
    OpenTunnel {
        port: u16,
        token: String,
    },
    /// Puts the thread on a shelf on the serving machine, whose store and sidebar change (1.6): settled, snoozed
    /// until `snoozed_until` (Unix ms), or neither (back in the list). Both at once is a bad request: a thread is on
    /// one shelf at a time. Setting the whole state, not a toggle, lets an undo put back exactly what was there.
    ShelveThread {
        thread_id: i64,
        settled: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snoozed_until: Option<i64>,
    },
    /// Every listed project and thread now, answered with [`Reply::Shell`], then each change after it pushed on
    /// this connection as [`ServerMessage::Shell`] (1.7). Subscribing again replaces the stream with a new snapshot:
    /// that is how a client recovers from a gap or a reconnect.
    SubscribeShell,
    /// An agent on the calling machine delegates here (1.8). Answered with [`Reply::Delegated`].
    Delegate {
        caller: Delegator,
        op: DelegateOp,
    },
}

/// What a remote user may answer a permission request with (1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionAnswer {
    AllowOnce,
    /// Allow it and its like while the agent runs.
    AllowThread,
    Deny,
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
    /// A change to the listed projects and threads (1.7), pushed only after this connection's `subscribe_shell`
    /// was answered, in seq order.
    Shell(ShellChange),
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
    /// `forward_port` succeeded (1.5): tunnels and `stop_forward` name the forward by this token.
    Forwarded {
        token: String,
    },
    /// The answer to `subscribe_shell` (1.7).
    Shell(ShellSnapshot),
    /// A `delegate` call's answer (1.8): the same JSON the local tool would give.
    Delegated(serde_json::Value),
    /// For calls with nothing to return (`send_message`, `interrupt`, `answer_permission`, `answer_questions`,
    /// `stop_forward`, `open_tunnel`, `shelve_thread`).
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
    /// Hello from a device key the user hasn't approved on that machine (1.2). The connection closes; the user
    /// is asked there.
    NotApproved,
    /// Hello from a device key the user turned down on that machine (1.2). The connection closes.
    Denied,
    /// A `delegate` call this machine doesn't allow (1.8): no link, a login that doesn't match, another provider.
    DelegationNotAllowed,
    /// A code from a newer minor version.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub path: String,
    /// Unix ms (1.7). 0 from an older server.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub created_at: i64,
}

fn is_zero(n: &i64) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Pinned on the serving machine (1.4): listed first.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    /// The agent's last reply as stored (markdown), cut to [`PREVIEW_CHARS`] (1.4). `None` before any reply, or
    /// from an older server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// When it was settled on the serving machine (Unix ms, 1.6). `None` when it isn't, or from an older server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settled_at: Option<i64>,
    /// Snoozed until then (Unix ms, 1.6): hidden while ahead, woke once passed. `None` from an older server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<i64>,
    /// Unix milliseconds.
    pub created_at: i64,
    pub updated_at: i64,
    /// The rest of the serving machine's row (1.7), so a client's sidebar draws what that machine's own does.
    /// Empty or absent from an older server. Paths are the serving machine's.
    #[serde(default, flatten)]
    pub row: ThreadRow,
}

/// A thread row's fields beyond the 1.0–1.6 ones (1.7). Each is left out of the JSON when empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadRow {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cwd: String,
    /// Which tool calls its agent may make without asking; the agent layer defines the values.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub permission_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    /// `open`, `draft`, `merged` or `closed`, as last read there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_state: Option<String>,
    /// The thread whose agent delegated this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_thread_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<i64>,
    /// A turn finished there while nobody looked at it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unread: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_of: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compare_group: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_task_id: Option<i64>,
    /// Usage limited: it continues by itself then (Unix ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_resume_at: Option<i64>,
    /// Started by an agent on another machine (1.8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_from: Option<DelegatedFrom>,
}

/// How much of a thread's last reply [`Thread::preview`] carries: enough for a sidebar row's two lines.
pub const PREVIEW_CHARS: usize = 400;

/// A tool result's image block as one line: "Image (205 KB)", plus " not kept" once the serving machine's store
/// dropped its bytes (it leaves `omitted_bytes` in their place). Shared by every client's timeline.
pub fn image_label(block: &serde_json::Value) -> String {
    let source = &block["source"];
    let omitted = source["omitted_bytes"]
        .as_u64()
        .or(block["omitted_bytes"].as_u64());
    let data = source["data"].as_str().or(block["data"].as_str());
    // Base64 is 4 characters per 3 bytes.
    let Some(bytes) = omitted.or(data.map(|d| d.len() as u64 / 4 * 3)) else {
        return "Image".to_owned();
    };
    let size = match bytes {
        b if b >= 1024 * 1024 => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
        b => format!("{} KB", ((b + 512) / 1024).max(1)),
    };
    let kept = if omitted.is_some() { " not kept" } else { "" };
    format!("Image ({size}){kept}")
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
