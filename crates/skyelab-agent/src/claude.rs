//! Claude's side of turn accounting: each prompt carries a `uuid`, and a `result` names the prompts it answered in
//! `user_message_uuids`. A prompt sent mid-turn can be folded into the running turn (one result for two prompts),
//! and a background wake ends with a result for none of ours.

use crate::Event;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// The uuids of prompts sent and not answered yet, shared by the session and its reader thread.
pub(crate) type Sent = Arc<Mutex<HashSet<String>>>;

fn sent(s: &Sent) -> std::sync::MutexGuard<'_, HashSet<String>> {
    s.lock().unwrap_or_else(|e| e.into_inner()) // a set of ids, consistent after a panic
}

/// The stream-json line for a user prompt, recorded in `pending`.
pub(crate) fn user_message(text: &str, pending: &Sent) -> Value {
    let uuid = uuid::Uuid::new_v4().to_string();
    sent(pending).insert(uuid.clone());
    json!({
        "type": "user",
        "message": { "role": "user", "content": text },
        "parent_tool_use_id": null,
        "session_id": "",
        "uuid": uuid,
    })
}

/// One stdout line. A result that lists prompt uuids answers those of ours it names; one without the list (an
/// older CLI) answers one.
pub(crate) fn parse(line: &str, pending: &Sent) -> Event {
    let mut event = Event::parse(line);
    if let Event::Result { prompts, .. } = &mut event
        && let Ok(v) = serde_json::from_str::<Value>(line)
    {
        let listed: Option<Vec<&str>> = match (&v["user_message_uuids"], &v["user_message_uuid"]) {
            (Value::Array(ids), _) => Some(ids.iter().filter_map(Value::as_str).collect()),
            (_, Value::String(id)) => Some(vec![id.as_str()]),
            _ => None,
        };
        if let Some(listed) = listed {
            let mut pending = sent(pending);
            let ours = listed.iter().filter(|id| pending.remove(**id)).count();
            *prompts = u32::try_from(ours).unwrap_or(u32::MAX);
        }
    }
    event
}
