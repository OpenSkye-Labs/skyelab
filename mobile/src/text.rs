//! What the screens say: an event's text and the connection line.

use crate::source::Connection;
use gpui::SharedString;
use skyelab_protocol::ThreadEvent;

/// What an event says: a prompt's text, an agent turn's text and tool calls, a result, else the raw payload.
pub fn event_text(e: &ThreadEvent) -> String {
    use serde_json::Value;
    let Ok(mut v) = serde_json::from_str::<Value>(&e.payload_json) else {
        return e.payload_json.clone();
    };
    // Agent rows are `skyelab_agent::Event`, serde's default externally tagged enum: `{"Assistant": {..}}`.
    if let Some((_, inner)) = v
        .as_object()
        .filter(|o| o.len() == 1)
        .and_then(|o| o.iter().next())
        && inner.is_object()
    {
        v = inner.clone();
    }
    if let Some(prompt) = v.as_str() {
        return prompt.to_owned();
    }
    if let Some((tool, what)) = permission(&e.payload_json) {
        return format!("Asks to use {tool}: {what}");
    }
    if let Some(decision) = v.get("decision").and_then(Value::as_str) {
        return match decision {
            "allow_once" => "Allowed once",
            "allow_thread" => "Allowed for this thread",
            "deny" => "Denied",
            _ => "Withdrawn",
        }
        .to_owned();
    }
    if let Some(blocks) = v.get("content").and_then(Value::as_array) {
        let parts: Vec<String> = blocks
            .iter()
            .filter_map(|b| match b.get("type")?.as_str()? {
                "text" => b.get("text")?.as_str().map(str::to_owned),
                "tool_use" => {
                    let name = b.get("name")?.as_str()?;
                    Some(match b.pointer("/input/command").and_then(Value::as_str) {
                        Some(command) => format!("→ {name}: {command}"),
                        None => format!("→ {name}"),
                    })
                }
                "tool_result" => Some("(tool output)".to_owned()),
                _ => None,
            })
            .collect();
        if !parts.is_empty() {
            return parts.join("\n");
        }
    }
    // `{"Stderr": {"text"}}`: the agent's own output after a failure.
    if let Some(text) = v.get("text").and_then(Value::as_str) {
        return text.to_owned();
    }
    if let Some(result) = v.get("result").and_then(Value::as_str) {
        return result.to_owned();
    }
    e.payload_json.clone()
}

/// A `{"Permission": {..}}` agent row's tool and a one-line summary of what it wants to do.
fn permission(payload: &str) -> Option<(String, String)> {
    use serde_json::Value;
    let v: Value = serde_json::from_str(payload).ok()?;
    let p = v.get("Permission")?;
    let tool = p.get("tool")?.as_str()?.to_owned();
    let input = p.get("input").cloned().unwrap_or(Value::Null);
    let what = ["command", "file_path", "path"]
        .iter()
        .find_map(|k| input.get(k).and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| input.to_string());
    let what: String = what
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect();
    Some((tool, what))
}

/// The permission requests in `events` nobody has answered yet: (request id, what it asks), oldest first.
pub fn pending_permissions(events: &[ThreadEvent]) -> Vec<(String, String)> {
    use serde_json::Value;
    let mut pending: Vec<(String, String)> = Vec::new();
    for e in events.iter().filter(|e| e.kind == "agent") {
        let Ok(v) = serde_json::from_str::<Value>(&e.payload_json) else {
            continue;
        };
        if let Some((tool, what)) = permission(&e.payload_json)
            && let Some(id) = v.pointer("/Permission/id").and_then(Value::as_str)
        {
            pending.push((id.to_owned(), format!("{tool}: {what}")));
        } else if let Some(id) = v.pointer("/PermissionResolved/id").and_then(Value::as_str) {
            pending.retain(|(p, _)| p != id);
        }
    }
    pending
}

/// The connection line under the title, and whether it is a problem.
pub(crate) fn connection_text(connection: &Connection) -> (SharedString, bool) {
    match connection {
        Connection::Connecting => ("Connecting…".into(), false),
        Connection::Connected => ("Connected".into(), false),
        Connection::Reconnecting(why) => (format!("Reconnecting… ({why})").into(), true),
        Connection::Refused => (
            "That machine didn't accept this device. Sign this phone in to your tailnet as the same user, \
             untagged, and check Tailscale is running on that machine."
                .into(),
            true,
        ),
        Connection::NodeChanged { was, now } => {
            let now = now.as_deref().unwrap_or("none");
            (
                format!(
                    "This isn't the machine you connected to before (node {now}, trusted: {was}). Trust it only \
                     if you replaced or renamed that machine."
                )
                .into(),
                true,
            )
        }
        Connection::Unverified(why) => (
            format!(
                "Can't check which machine this is ({why}). Nothing was sent to it. If the saved machines file \
                 is damaged, reinstall the app to forget them."
            )
            .into(),
            true,
        ),
        Connection::VersionMismatch(why) => (
            format!("That machine runs another SkyeLab version ({why}). Update both to the same release.")
                .into(),
            true,
        ),
        Connection::Stopped(why) => (
            format!("Disconnected ({why}). Go back and connect again.").into(),
            true,
        ),
    }
}
