//! The tools, their schemas, and input validation. Agent input is untrusted: every field is checked here.

use super::browser::{self, BrowserTool};
use super::device::{self, DeviceTool};
use serde_json::{Map, Value, json};

/// Longest prompt or message an agent may send.
const MAX_TEXT: usize = 100_000;
/// `wait_thread` waits at most this long.
const MAX_WAIT_S: u64 = 300;
const MAX_REQUEST_ID: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tool {
    ListProjects,
    StartThread {
        project_id: i64,
        prompt: String,
        worktree: bool,
        /// The agent's own name for this start: a retry with the same one gets the same thread.
        request_id: Option<String>,
    },
    ThreadStatus {
        thread_id: i64,
    },
    ReadThread {
        thread_id: i64,
        since_seq: i64,
    },
    SendMessage {
        thread_id: i64,
        text: String,
    },
    WaitThread {
        thread_id: i64,
        timeout_s: u64,
    },
    Browser(BrowserTool),
    Device(DeviceTool),
}

pub(super) fn list() -> Value {
    let id = |what: &str| json!({"type": "integer", "minimum": 1, "description": what});
    let thread = id("A thread id, from start_thread.");
    let mut tools = json!([
        {
            "name": "list_projects",
            "description": "Projects you can delegate to: those bound to the same account as you.",
            "inputSchema": {"type": "object", "properties": {}},
        },
        {
            "name": "start_thread",
            "description": "Start a new agent thread in a project with a prompt. It runs under the project's \
                bound account, which must be yours. Returns its thread_id as soon as the thread exists; its \
                agent may still be starting (see thread_status). Pass a request_id of your own and use the same \
                one if you retry, so a retry after a timeout returns the same thread instead of starting another (the \
                first call's project and prompt win).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": id("A project id from list_projects."),
                    "prompt": {"type": "string", "minLength": 1, "maxLength": MAX_TEXT},
                    "worktree": {"type": "boolean", "description": "Run in its own git worktree."},
                    "request_id": {"type": "string", "minLength": 1, "maxLength": MAX_REQUEST_ID,
                        "description": "Any string unique to this start; repeat it on a retry."},
                },
                "required": ["project_id", "prompt"],
            },
        },
        {
            "name": "thread_status",
            "description": "A thread's title, status and account.",
            "inputSchema": {"type": "object", "properties": {"thread_id": thread}, "required": ["thread_id"]},
        },
        {
            "name": "read_thread",
            "description": "The assistant text and final result after since_seq. Pass the returned last_seq \
                next time to read only what's new.",
            "inputSchema": {
                "type": "object",
                "properties": {"thread_id": thread, "since_seq": {"type": "integer", "minimum": 0}},
                "required": ["thread_id"],
            },
        },
        {
            "name": "send_message",
            "description": "Send a follow-up message to a running thread.",
            "inputSchema": {
                "type": "object",
                "properties": {"thread_id": thread, "text": {"type": "string", "minLength": 1, "maxLength": MAX_TEXT}},
                "required": ["thread_id", "text"],
            },
        },
        {
            "name": "wait_thread",
            "description": "Wait until the thread finishes its turn, or timeout_s passes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "thread_id": thread,
                    "timeout_s": {"type": "integer", "minimum": 1, "maximum": MAX_WAIT_S},
                },
                "required": ["thread_id", "timeout_s"],
            },
        },
    ]);
    if let Value::Array(list) = &mut tools {
        list.extend(browser::list());
        list.extend(device::list());
    }
    tools
}

/// Parses a `tools/call`. The error names the bad field.
pub(super) fn parse(name: &str, args: &Value) -> Result<Tool, String> {
    let empty = Map::new();
    let args = match args {
        Value::Null => &empty,
        Value::Object(m) => m,
        _ => return Err(format!("{name}: arguments must be an object")),
    };
    Ok(match name {
        "list_projects" => Tool::ListProjects,
        "start_thread" => Tool::StartThread {
            project_id: id(args, "project_id")?,
            prompt: text(args, "prompt")?,
            worktree: match args.get("worktree") {
                None => false,
                Some(Value::Bool(b)) => *b,
                Some(_) => return Err("worktree must be true or false".into()),
            },
            request_id: match args.get("request_id") {
                None => None,
                Some(_) => Some(bounded(args, "request_id", MAX_REQUEST_ID)?),
            },
        },
        "thread_status" => Tool::ThreadStatus {
            thread_id: id(args, "thread_id")?,
        },
        "read_thread" => Tool::ReadThread {
            thread_id: id(args, "thread_id")?,
            since_seq: match args.get("since_seq") {
                None => 0,
                Some(v) => v
                    .as_i64()
                    .filter(|n| *n >= 0)
                    .ok_or("since_seq must be an integer >= 0")?,
            },
        },
        "send_message" => Tool::SendMessage {
            thread_id: id(args, "thread_id")?,
            text: text(args, "text")?,
        },
        "wait_thread" => Tool::WaitThread {
            thread_id: id(args, "thread_id")?,
            timeout_s: args
                .get("timeout_s")
                .and_then(Value::as_u64)
                .filter(|s| (1..=MAX_WAIT_S).contains(s))
                .ok_or(format!(
                    "timeout_s must be an integer from 1 to {MAX_WAIT_S}"
                ))?,
        },
        other => match (browser::parse(other, args), device::parse(other, args)) {
            (Some(tool), _) => Tool::Browser(tool?),
            (_, Some(tool)) => Tool::Device(tool?),
            _ => return Err(format!("unknown tool '{other}'")),
        },
    })
}

fn id(args: &Map<String, Value>, field: &str) -> Result<i64, String> {
    args.get(field)
        .and_then(Value::as_i64)
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("{field} must be a positive integer"))
}

fn text(args: &Map<String, Value>, field: &str) -> Result<String, String> {
    bounded(args, field, MAX_TEXT)
}

/// A non-blank string `field` of at most `max` characters.
pub(super) fn bounded(
    args: &Map<String, Value>,
    field: &str,
    max: usize,
) -> Result<String, String> {
    let s = args
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{field} must be a string"))?;
    if s.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if s.chars().count() > max {
        return Err(format!("{field} is longer than {max} characters"));
    }
    Ok(s.to_owned())
}
