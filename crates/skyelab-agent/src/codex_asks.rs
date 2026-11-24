//! Codex's approval requests: what the user's answer sends back, and the asks SkyeLab answers by a thread rule.

use super::*;

impl Codex {
    /// The reply to approval request `id`, or `None` when it isn't waiting. It stays open until [`Self::forget`].
    pub(crate) fn answer(&mut self, id: &str, decision: Decision) -> Option<String> {
        let rpc = self.asks.get(id)?;
        if self.questions.contains_key(id) {
            return Some(json!({ "id": rpc, "result": { "answers": {} } }).to_string());
        }
        if decision == Decision::AllowThread {
            self.allow.allow_exact(id);
        }
        let decision = match decision {
            Decision::AllowOnce => "accept",
            Decision::AllowThread => "acceptForSession",
            Decision::Deny | Decision::Withdrawn => "decline",
        };
        Some(json!({ "id": rpc, "result": { "decision": decision } }).to_string())
    }

    /// Allows waiting shell ask `id` and, from now on, every plain command starting with `prefix` (this thread only):
    /// the reply and the rule's label, or why not.
    pub(crate) fn allow_prefix(
        &mut self,
        id: &str,
        prefix: &str,
    ) -> Result<(String, String), String> {
        let rpc = self
            .asks
            .get(id)
            .ok_or("The agent is no longer waiting for that answer.")?;
        let line = json!({ "id": rpc, "result": { "decision": "accept" } }).to_string();
        Ok((line, self.allow.allow_prefix(id, prefix)?))
    }

    pub(crate) fn forget(&mut self, id: &str) {
        self.allow.forget(id);
        self.asks.remove(id);
        self.questions.remove(id);
    }

    /// Every open request, withdrawn: its turn or its process is over.
    pub(crate) fn withdraw_all(&mut self) -> Vec<Event> {
        self.questions.clear();
        self.patches.clear();
        self.allow.clear_waiting();
        std::mem::take(&mut self.asks)
            .into_keys()
            .map(|id| Event::PermissionResolved {
                id,
                decision: Decision::Withdrawn,
                rule: None,
            })
            .collect()
    }

    /// A server request: an approval waits for the user; anything else is answered now.
    pub(super) fn on_request(
        &mut self,
        method: &str,
        rpc: Value,
        params: &Value,
    ) -> (Vec<Event>, Vec<String>) {
        // A secret answer would be typed into a plain field, so those stay unsupported.
        let secret = params["questions"]
            .as_array()
            .is_some_and(|qs| qs.iter().any(|q| q["isSecret"] == true));
        if method == "item/tool/requestUserInput" && params["questions"].is_array() && !secret {
            return self.on_question(rpc, params);
        }
        // Only the fields codex sent: an absent `grantRoot` must not show as null.
        let input: serde_json::Map<String, Value> = match method {
            // Every field but the bookkeeping, so one that changes what running it means (network, permissions, a
            // newer Codex's) reaches the card and keeps a rule from answering it.
            "item/commandExecution/requestApproval" => params
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(k, v)| !v.is_null() && !allow::bookkeeping(k, v))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            "item/fileChange/requestApproval" => ["grantRoot", "reason"]
                .into_iter()
                .filter(|k| !params[*k].is_null())
                .map(|k| (k.to_owned(), params[k].clone()))
                .collect(),
            _ => return (Vec::new(), vec![answer(method, rpc)]),
        };
        let tool = match method {
            "item/commandExecution/requestApproval" => allow::SHELL,
            _ => "apply_patch",
        };
        let mut input = Value::Object(input);
        // What the patch does, so the card never asks to approve an edit it can't show.
        if let Some(changes) = params["itemId"].as_str().and_then(|i| self.patches.get(i)) {
            input["changes"] = changes.clone();
        }
        let id = format!("{}codex-{}", self.nonce, rpc);
        // A subagent's ask (another thread) is never this thread's to allow.
        let own = params["threadId"]
            .as_str()
            .is_none_or(|t| self.thread_id.as_deref() == Some(t));
        if tool == allow::SHELL {
            if let Some(rule) = self.allow.covers(&input).filter(|_| own) {
                let reply = json!({ "id": rpc, "result": { "decision": "accept" } }).to_string();
                let resolved = Event::PermissionResolved {
                    id: id.clone(),
                    decision: Decision::AllowThread,
                    rule: Some(rule),
                };
                let ask = Event::Permission {
                    id,
                    tool: tool.to_owned(),
                    input,
                };
                return (vec![ask, resolved], vec![reply]);
            }
            self.allow.wait(id.clone(), input.clone());
        }
        self.asks.insert(id.clone(), rpc);
        let event = Event::Permission {
            id,
            tool: tool.to_owned(),
            input,
        };
        (vec![event], Vec::new())
    }
}
