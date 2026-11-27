//! The tools, their schemas, and input validation. Agent input is untrusted: every field is checked here.

use super::browser::{self, BrowserTool};
use super::device::{self, DeviceTool};
use super::html::{self, HtmlTool};
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
    Html(HtmlTool),
    /// The machines this agent may delegate to.
    ListMachines,
    /// One of the delegation tools above, sent to `machine` (a MagicDNS name from `list_machines`).
    Remote {
        machine: String,
        tool: Box<Tool>,
    },
}

/// The tools that take an optional `machine`.
const REMOTE_TOOLS: [&str; 6] = [
    "list_projects",
    "start_thread",
    "thread_status",
    "read_thread",
    "send_message",
    "wait_thread",
];

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
            "description": "Wait until the thread finishes its turn, or timeout_s passes. Once done, `turn` gives its \
                last finished turn as data: is_error, summary (its result text), duration_ms, cost_usd and \
                files_changed.",
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
        for tool in list
            .iter_mut()
            .filter(|t| REMOTE_TOOLS.contains(&t["name"].as_str().unwrap_or("")))
        {
            tool["inputSchema"]["properties"]["machine"] = json!({"type": "string", "maxLength": 253,
                "description": "Another of the user's machines, from list_machines. Without it, this machine. \
                    With it, thread ids are that machine's."});
        }
        list.push(json!({
            "name": "list_machines",
            "description": "The user's other machines you may delegate to (pass one as `machine` to the other \
                tools), each with its state: ready, offline, needs_update or not_linked_here.",
            "inputSchema": {"type": "object", "properties": {}},
        }));
        list.extend(browser::list());
        list.extend(device::list());
        list.extend(html::list());
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
    if let Some(machine) = args.get("machine").filter(|_| REMOTE_TOOLS.contains(&name)) {
        let machine = machine
            .as_str()
            .map(|m| m.trim_end_matches('.').to_ascii_lowercase())
            .filter(|m| skyelab_protocol::is_magicdns_name(m))
            .ok_or("machine must be a machine name from list_machines (machine.tailnet.ts.net)")?;
        let mut rest = args.clone();
        rest.remove("machine");
        let tool = Box::new(parse(name, &Value::Object(rest))?);
        return Ok(Tool::Remote { machine, tool });
    }
    Ok(match name {
        "list_machines" => Tool::ListMachines,
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
        other => match (
            browser::parse(other, args),
            device::parse(other, args),
            html::parse(other, args),
        ) {
            (Some(tool), _, _) => Tool::Browser(tool?),
            (_, Some(tool), _) => Tool::Device(tool?),
            (_, _, Some(tool)) => Tool::Html(tool?),
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

#[cfg(test)]
mod tests {
    use super::{Tool, list, parse};
    use serde_json::json;

    #[test]
    fn machine_routes_the_six_delegation_tools() {
        let local = parse("thread_status", &json!({"thread_id": 3})).expect("local");
        assert_eq!(local, Tool::ThreadStatus { thread_id: 3 });
        let remote = parse(
            "thread_status",
            &json!({"thread_id": 3, "machine": "Desk.Example.ts.net."}),
        )
        .expect("remote");
        assert_eq!(
            remote,
            Tool::Remote {
                machine: "desk.example.ts.net".into(),
                tool: Box::new(Tool::ThreadStatus { thread_id: 3 }),
            }
        );
        for bad in ["localhost", "100.64.0.1", "desk", "-x.a.ts.net"] {
            assert!(
                parse("list_projects", &json!({"machine": bad})).is_err(),
                "{bad}"
            );
        }
        assert!(
            parse("thread_status", &json!({"machine": "d.e.ts.net"})).is_err(),
            "still validated"
        );
        assert_eq!(
            parse("list_machines", &json!({})).expect("parsed"),
            Tool::ListMachines
        );
        let tools = list();
        let names: Vec<&str> = tools
            .as_array()
            .expect("list")
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert!(names.contains(&"list_machines"));
        let start = tools
            .as_array()
            .expect("list")
            .iter()
            .find(|t| t["name"] == "start_thread")
            .expect("start");
        assert!(start["inputSchema"]["properties"]["machine"].is_object());
    }
}
