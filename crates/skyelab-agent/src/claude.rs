//! Claude's side of turn accounting: each prompt carries a `uuid`, and a `result` names the prompts it answered in
//! `user_message_uuids`. A prompt sent mid-turn can be folded into the running turn (one result for two prompts),
//! and a background wake (`origin.kind` `task-notification`, no uuids) answers none of ours.

use crate::{Decision, Event};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

/// The uuids of prompts sent and not answered yet, oldest first, shared by the session and its reader thread.
pub(crate) type Sent = Arc<Mutex<VecDeque<String>>>;

fn sent(s: &Sent) -> std::sync::MutexGuard<'_, VecDeque<String>> {
    s.lock().unwrap_or_else(|e| e.into_inner()) // a set of ids, consistent after a panic
}

/// Permission asks (`can_use_tool` control requests) not answered yet. The id an [`Event::Permission`] carries is
/// `nonce` + the CLI's request id: the nonce is new per process, so an ask stored by an earlier process can never
/// name one of this process's.
#[derive(Default)]
pub(crate) struct Book {
    nonce: String,
    open: BTreeMap<String, Ask>,
}

impl Book {
    pub(crate) fn new(nonce: String) -> Asks {
        Arc::new(Mutex::new(Book {
            nonce,
            open: BTreeMap::new(),
        }))
    }
}

pub(crate) type Asks = Arc<Mutex<Book>>;

struct Ask {
    request_id: String,
    tool: String,
    input: Value,
    suggestions: Value,
}

pub(crate) fn asks(a: &Asks) -> std::sync::MutexGuard<'_, Book> {
    a.lock().unwrap_or_else(|e| e.into_inner()) // plain data, consistent after a panic
}

/// The `control_response` line answering ask `id` with `decision`, or `None` when `id` isn't open. The ask stays
/// open until [`forget`]: the line may still fail to reach the agent.
pub(crate) fn answer(open: &Asks, id: &str, decision: Decision) -> Option<Value> {
    let book = asks(open);
    let ask = book.open.get(id)?;
    let response = match decision {
        Decision::AllowOnce => json!({ "behavior": "allow", "updatedInput": ask.input }),
        Decision::AllowThread => json!({
            "behavior": "allow",
            "updatedInput": ask.input,
            "updatedPermissions": session_rules(ask),
        }),
        Decision::Deny | Decision::Withdrawn => {
            json!({ "behavior": "deny", "message": "The user denied this in SkyeLab." })
        }
    };
    Some(json!({
        "type": "control_response",
        "response": { "subtype": "success", "request_id": ask.request_id, "response": response },
    }))
}

pub(crate) fn forget(open: &Asks, id: &str) {
    asks(open).open.remove(id);
}

/// What "Allow for this thread" adds: only Claude's suggested allow rules, kept to this process (never written to
/// a settings file). Suggestions that change the mode, add directories or replace rules are dropped. With none left,
/// a rule for this exact command, or for the tool when it has no command.
fn session_rules(ask: &Ask) -> Vec<Value> {
    let mut rules: Vec<Value> = ask
        .suggestions
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["type"] == "addRules" && r["behavior"] == "allow" && r["rules"].is_array())
        .map(|r| json!({ "type": "addRules", "rules": r["rules"], "behavior": "allow", "destination": "session" }))
        .collect();
    if rules.is_empty() {
        let rule = match ask.input["command"].as_str() {
            Some(command) => json!({ "toolName": ask.tool, "ruleContent": command }),
            None => json!({ "toolName": ask.tool }),
        };
        rules.push(json!({ "type": "addRules", "rules": [rule], "behavior": "allow", "destination": "session" }));
    }
    rules
}

/// The stream-json line for a user prompt, recorded in `pending`.
pub(crate) fn user_message(text: &str, attachments: &[crate::Attachment], pending: &Sent) -> Value {
    let uuid = uuid::Uuid::new_v4().to_string();
    sent(pending).push_back(uuid.clone());
    json!({
        "type": "user",
        "message": { "role": "user", "content": crate::attach::claude_content(text, attachments) },
        "parent_tool_use_id": null,
        "session_id": "",
        "uuid": uuid,
    })
}

/// One stdout line. A result that lists prompt uuids answers those of ours it names. Without the list, one from
/// another origin than the user (a background wake, a peer) answers none, and any other answers the oldest prompt
/// (an older CLI). Streaming chatter nobody shows yields nothing.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(crate) fn parse(line: &str, pending: &Sent, open: &Asks) -> Vec<Event> {
    if let Some(events) = control(line, open) {
        return events;
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return vec![Event::Unknown(Value::String(line.to_owned()))];
    };
    let gone = v["type"] == "result" && crate::event::names_gone_session(&v);
    let Some(mut event) = Event::from_claude(v.clone()) else {
        return Vec::new();
    };
    if let Some(reading) = crate::event::claude_context(&v) {
        return vec![event, reading];
    }
    if matches!(event, Event::RateLimit { .. }) {
        let mut events = usage_windows(&v);
        events.push(event);
        return events;
    }
    if let Event::Result { prompts, .. } = &mut event {
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
        // A turn of ours ended, so nothing it asked can be answered any more. A background wake ends none.
        let mut events = if answered > 0 {
            withdraw_all(open)
        } else {
            Vec::new()
        };
        if gone {
            events.push(Event::SessionGone);
        }
        events.push(event);
        events.extend(crate::event::claude_context_windows(&v));
        return events;
    }
    let mcp = matches!(event, Event::Init { .. })
        .then(|| crate::event::claude_mcp(&v))
        .flatten();
    std::iter::once(event).chain(mcp).collect()
}

/// One reading per window of a rate-limit line's `unifiedWindows` (Claude Code 2.1.331 reports utilization only
/// there). Readings never say limited: the line's own event, sent after them, does.
fn usage_windows(v: &Value) -> Vec<Event> {
    let Some(windows) = v["rate_limit_info"]["unifiedWindows"].as_object() else {
        return Vec::new();
    };
    windows
        .iter()
        .filter_map(|(window, w)| {
            let used = w["utilization"].as_f64().filter(|u| u.is_finite())?;
            Some(Event::RateLimit {
                limited: false,
                resets_at: w["resetsAt"].as_i64(),
                window: Some(window.clone()),
                used: Some(used.clamp(0.0, 1.0) as f32),
            })
        })
        .collect()
}

/// The `control_response` line answering AskUserQuestion ask `id`: `answers` holds each question's picked labels,
/// in question order. Claude reads them from `updatedInput.answers`, keyed by question text, labels joined by ", ".
/// `None` when `id` isn't an open AskUserQuestion ask or the answer count doesn't match its questions.
pub(crate) fn answer_questions(open: &Asks, id: &str, answers: &[Vec<String>]) -> Option<Value> {
    let book = asks(open);
    let ask = book.open.get(id).filter(|a| a.tool == QUESTION_TOOL)?;
    let questions = ask.input["questions"].as_array()?;
    if questions.len() != answers.len() {
        return None;
    }
    let map: serde_json::Map<String, Value> = questions
        .iter()
        .zip(answers)
        .map(|(q, a)| {
            let text = q["question"].as_str().unwrap_or_default().to_owned();
            (text, Value::String(a.join(", ")))
        })
        .collect();
    let response = json!({
        "behavior": "allow",
        "updatedInput": { "questions": ask.input["questions"], "answers": map },
    });
    Some(json!({
        "type": "control_response",
        "response": { "subtype": "success", "request_id": ask.request_id, "response": response },
    }))
}

/// The tool Claude proposes a plan with, in plan mode.
pub const PLAN_TOOL: &str = "ExitPlanMode";

/// The tool Claude asks the user questions with; Codex's user-input requests arrive under the same name.
pub const QUESTION_TOOL: &str = "AskUserQuestion";

/// Every open ask, withdrawn: its turn or its process is over.
pub(crate) fn withdraw_all(open: &Asks) -> Vec<Event> {
    std::mem::take(&mut asks(open).open)
        .into_keys()
        .map(|id| Event::PermissionResolved {
            id,
            decision: Decision::Withdrawn,
        })
        .collect()
}

/// A permission ask or its withdrawal, or `None` for any other line.
fn control(line: &str, open: &Asks) -> Option<Vec<Event>> {
    if !line.contains("control_") {
        return None; // most lines: skip the second parse
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let request_id = v["request_id"].as_str()?.to_owned();
    let mut book = asks(open);
    let id = format!("{}{request_id}", book.nonce);
    match v["type"].as_str()? {
        "control_request" if v["request"]["subtype"] == "can_use_tool" => {
            let r = &v["request"];
            let tool = r["tool_name"].as_str().unwrap_or("tool").to_owned();
            let input = r["input"].clone();
            book.open.insert(
                id.clone(),
                Ask {
                    request_id,
                    tool: tool.clone(),
                    input: input.clone(),
                    suggestions: r["permission_suggestions"].clone(),
                },
            );
            Some(vec![Event::Permission { id, tool, input }])
        }
        "control_cancel_request" => Some(
            book.open
                .remove(&id)
                .map(|_| Event::PermissionResolved {
                    id,
                    decision: Decision::Withdrawn,
                })
                .into_iter()
                .collect(),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_result_withdraws_open_asks() {
        let (sent, open) = (Sent::default(), Asks::default());
        let ask = r#"{"type":"control_request","request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{}}}"#;
        assert!(
            matches!(&parse(ask, &sent, &open)[..], [Event::Permission { id, .. }] if id == "r1")
        );
        let result = r#"{"type":"result","is_error":false,"duration_ms":1,"result":"ok"}"#;
        let events = parse(result, &sent, &open);
        assert_eq!(
            events[0],
            Event::PermissionResolved {
                id: "r1".into(),
                decision: Decision::Withdrawn
            }
        );
        assert!(matches!(events[1], Event::Result { .. }));
        assert!(answer(&open, "r1", Decision::AllowOnce).is_none());
        assert!(withdraw_all(&open).is_empty());
    }

    #[test]
    fn a_background_wake_keeps_the_turns_open_asks() {
        let (sent, open) = (Sent::default(), Asks::default());
        user_message("hello", &[], &sent);
        let ask = r#"{"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Read","input":{}}}"#;
        let id = match &parse(ask, &sent, &open)[..] {
            [Event::Permission { id, .. }] => id.clone(),
            other => panic!("{other:?}"),
        };
        let wake = r#"{"type":"result","is_error":false,"duration_ms":1,"result":"wake","origin":{"kind":"task-notification"}}"#;
        parse(wake, &sent, &open);
        assert!(
            answer(&open, &id, Decision::Deny).is_some(),
            "background wake discarded foreground approval"
        );
    }
}
