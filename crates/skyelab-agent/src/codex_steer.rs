//! Codex steering and the per-turn model pick: a prompt either queues for the next turn or, steered, goes into the
//! running one with `turn/steer`; a refused steer runs as its own turn, counted once.

use super::Codex;
use crate::Event;
use anyhow::Result;
use serde_json::{Value, json};

/// The model and effort every turn carries (`None`: Codex's default).
#[derive(Debug, Clone, Default)]
pub(crate) struct Pick {
    pub(crate) model: Option<String>,
    pub(crate) effort: Option<String>,
}

/// `initialize`'s params: who this client is.
pub(crate) fn initialize_params() -> Value {
    json!({ "clientInfo": {
        "name": "skyelab", "title": "SkyeLab", "version": env!("CARGO_PKG_VERSION"),
    }})
}

impl Codex {
    /// Queues a prompt behind the running turn, or with `steer` sends it into that turn (`turn/steer`) when one
    /// runs and its id is known.
    pub(crate) fn prompt(
        &mut self,
        text: &str,
        attachments: &[crate::Attachment],
        steer: bool,
    ) -> Result<Vec<String>> {
        if let Some(why) = &self.closed {
            anyhow::bail!("codex couldn't open this thread ({why}); start a new thread");
        }
        let input = crate::attach::codex_input(text, attachments)?;
        if steer && let (Some(thread), Some(turn)) = (self.thread_id.clone(), self.turn_id.clone())
        {
            self.answers = Some(self.answers.unwrap_or(1) + 1);
            self.steers
                .push((self.next_id, turn.clone(), input.clone()));
            return Ok(vec![self.request(
                "turn/steer",
                json!({ "threadId": thread, "expectedTurnId": turn, "input": input }),
            )]);
        }
        self.queue.push_back((input, false));
        Ok(self.pump())
    }

    /// The reply to a `turn/steer` request, or `None` when `id` isn't one.
    pub(super) fn on_steer_reply(
        &mut self,
        id: Option<u64>,
        refused: bool,
    ) -> Option<(Vec<Event>, Vec<String>)> {
        let ix = self.steers.iter().position(|(req, ..)| Some(*req) == id)?;
        let (_, turn, input) = self.steers.remove(ix);
        if !refused {
            return Some((Vec::new(), Vec::new()));
        }
        // Refused (the turn ended meanwhile): it runs as its own turn. If its turn still runs, that turn no
        // longer answers it; if that turn already finished, it was counted there.
        let still_running = self.turn_id.as_deref() == Some(turn.as_str());
        if still_running {
            self.answers = self.answers.map(|n| n.saturating_sub(1));
        }
        self.queue.push_back((input, !still_running));
        Some((Vec::new(), self.pump()))
    }
}
