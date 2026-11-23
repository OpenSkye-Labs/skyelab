//! Claude's side of turn accounting: each prompt carries a `uuid`, and a `result` names the prompts it answered in
//! `user_message_uuids`. A prompt sent mid-turn can be folded into the running turn (one result for two prompts),
//! and a background wake (`origin.kind` `task-notification`, no uuids) answers none of ours.

use crate::Event;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// The uuids of prompts sent and not answered yet, oldest first, shared by the session and its reader thread.
pub(crate) type Sent = Arc<Mutex<VecDeque<String>>>;

fn sent(s: &Sent) -> std::sync::MutexGuard<'_, VecDeque<String>> {
    s.lock().unwrap_or_else(|e| e.into_inner()) // a set of ids, consistent after a panic
}

/// The stream-json line for a user prompt, recorded in `pending`.
pub(crate) fn user_message(text: &str, pending: &Sent) -> Value {
    let uuid = uuid::Uuid::new_v4().to_string();
    sent(pending).push_back(uuid.clone());
    json!({
        "type": "user",
        "message": { "role": "user", "content": text },
        "parent_tool_use_id": null,
        "session_id": "",
        "uuid": uuid,
    })
}

/// One stdout line. A result that lists prompt uuids answers those of ours it names. Without the list, one from
/// another origin than the user (a background wake, a peer) answers none, and any other answers the oldest prompt
/// (an older CLI).
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
        let mut pending = sent(pending);
        let answered = match listed {
            Some(listed) => {
                let before = pending.len();
                pending.retain(|id| !listed.contains(&id.as_str()));
                before - pending.len()
            }
            None if v["origin"]["kind"].as_str().is_some_and(|k| k != "human") => 0,
            None => {
                pending.pop_front();
                1
            }
        };
        *prompts = u32::try_from(answered).unwrap_or(u32::MAX);
    }
    event
}
