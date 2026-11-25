//! Bounded ACP `session/request_permission` handling: permission cards, `interaction_*` questions,
//! nonce-prefixed ask IDs, open-ask caps, and EOF/turn-end withdrawal.

use crate::acp_updates::{sanitize_payload, truncate_utf8};
use crate::{Decision, Event, QUESTION_TOOL};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

/// Hard cap on simultaneously open permission asks per ACP session.
pub(crate) const MAX_OPEN_ASKS: usize = 64;
const MAX_OPTIONS: usize = 32;
const MAX_LABEL_BYTES: usize = 512;

#[derive(Clone)]
struct PermOption {
    option_id: String,
    name: String,
    kind: String,
}

struct PendingAsk {
    rpc_id: Value,
    options: Vec<PermOption>,
    is_question: bool,
}

#[derive(Default)]
pub(crate) struct Asks {
    pub(crate) nonce: String,
    open: BTreeMap<String, PendingAsk>,
}

impl Asks {
    pub(crate) fn new(nonce: String) -> Self {
        Self {
            nonce,
            open: BTreeMap::new(),
        }
    }

    pub(crate) fn on_request(
        &mut self,
        rpc_id: Value,
        params: &Value,
    ) -> (Vec<Event>, Vec<String>) {
        if self.open.len() >= MAX_OPEN_ASKS {
            return (Vec::new(), vec![cancelled_reply(&rpc_id)]);
        }
        let options = parse_options(&params["options"]);
        let id = format!("{}acp-{}", self.nonce, rpc_id);
        let tool_call = &params["toolCall"];
        let call_id = tool_call["toolCallId"].as_str().unwrap_or_default();

        if call_id.starts_with("interaction_") && !options.is_empty() && unique_option_ids(&options)
        {
            let question = tool_call["title"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| truncate_utf8(s, 4096))
                .unwrap_or_else(|| "Choose an option.".to_owned());
            let q_opts: Vec<Value> = options
                .iter()
                .map(|o| {
                    let label = if o.name.is_empty() {
                        &o.option_id
                    } else {
                        &o.name
                    };
                    json!({ "label": label, "description": label })
                })
                .collect();
            let input = json!({
                "questions": [{
                    "header": "Question",
                    "question": question,
                    "multiSelect": false,
                    "options": q_opts,
                }]
            });
            self.open.insert(
                id.clone(),
                PendingAsk {
                    rpc_id,
                    options,
                    is_question: true,
                },
            );
            return (
                vec![Event::Permission {
                    id,
                    tool: QUESTION_TOOL.to_owned(),
                    input,
                }],
                Vec::new(),
            );
        }

        let tool = tool_call["title"]
            .as_str()
            .or_else(|| tool_call["kind"].as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| truncate_utf8(s, 256))
            .unwrap_or_else(|| "Tool".to_owned());
        let input = if !tool_call["rawInput"].is_null() {
            sanitize_payload(&tool_call["rawInput"])
        } else {
            json!({ "title": tool })
        };
        self.open.insert(
            id.clone(),
            PendingAsk {
                rpc_id,
                options,
                is_question: false,
            },
        );
        (vec![Event::Permission { id, tool, input }], Vec::new())
    }

    pub(crate) fn answer(&self, id: &str, decision: Decision) -> Option<String> {
        let ask = self.open.get(id)?;
        if ask.is_question {
            return Some(cancelled_reply(&ask.rpc_id));
        }
        let wanted: &[&str] = match decision {
            Decision::AllowOnce => &["allow_once"],
            Decision::AllowThread => &["allow_always", "allow_once"],
            Decision::Deny => &["reject_once"],
            Decision::Withdrawn => return Some(cancelled_reply(&ask.rpc_id)),
        };
        for kind in wanted {
            if let Some(opt) = ask.options.iter().find(|o| o.kind == *kind) {
                return Some(selected_reply(&ask.rpc_id, &opt.option_id));
            }
        }
        Some(cancelled_reply(&ask.rpc_id))
    }

    pub(crate) fn answer_questions(&self, id: &str, answers: &[Vec<String>]) -> Option<String> {
        let ask = self.open.get(id).filter(|a| a.is_question)?;
        if answers.len() != 1 {
            return None;
        }
        let picked = answers[0].first()?.trim();
        let exact = ask
            .options
            .iter()
            .find(|o| o.option_id == picked)
            .or_else(|| ask.options.iter().find(|o| o.name == picked))?;
        Some(selected_reply(&ask.rpc_id, &exact.option_id))
    }

    pub(crate) fn forget(&mut self, id: &str) {
        self.open.remove(id);
    }

    pub(crate) fn withdraw_all(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.open)
            .into_keys()
            .map(|id| Event::PermissionResolved {
                id,
                decision: Decision::Withdrawn,
                rule: None,
            })
            .collect()
    }
}

fn parse_options(v: &Value) -> Vec<PermOption> {
    v.as_array()
        .into_iter()
        .flatten()
        .take(MAX_OPTIONS)
        .filter_map(|o| {
            let option_id = o["optionId"].as_str()?.trim();
            if option_id.is_empty() {
                return None;
            }
            Some(PermOption {
                option_id: truncate_utf8(option_id, 256),
                name: truncate_utf8(
                    o["name"].as_str().unwrap_or_default().trim(),
                    MAX_LABEL_BYTES,
                ),
                kind: truncate_utf8(o["kind"].as_str().unwrap_or_default().trim(), 64),
            })
        })
        .collect()
}

fn unique_option_ids(options: &[PermOption]) -> bool {
    let mut seen = HashSet::new();
    options.iter().all(|o| seen.insert(o.option_id.as_str()))
}

pub(crate) fn selected_reply(rpc_id: &Value, option_id: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": rpc_id,
        "result": {
            "outcome": {
                "outcome": "selected",
                "optionId": option_id,
            }
        }
    })
    .to_string()
}

pub(crate) fn cancelled_reply(rpc_id: &Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": rpc_id,
        "result": {
            "outcome": {
                "outcome": "cancelled",
            }
        }
    })
    .to_string()
}
