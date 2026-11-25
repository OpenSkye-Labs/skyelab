//! What a running session is sent besides control: prompts (with files), steers into the running turn, and a new
//! model for the next turns.

use crate::{Attachment, Session, attach, claude, codex, lock, models};
use anyhow::{Result, bail};
use serde_json::json;

impl Session {
    /// Sends a follow-up user turn. Codex queues it behind a running turn, as `claude` does.
    pub fn send_user_message(&mut self, text: &str) -> Result<()> {
        self.send_message(text, &[])
    }

    /// [`send_user_message`](Self::send_user_message) with files attached.
    pub fn send_message(&mut self, text: &str, attachments: &[Attachment]) -> Result<()> {
        attach::check_count(attachments)?;
        if self.cursor.is_some() {
            bail!("Cursor runs one turn per process; start a resumed turn for the next message");
        }
        if let Some(state) = &self.codex {
            let lines = lock(state).prompt(text, attachments, false)?;
            return lines.into_iter().try_for_each(|l| self.queue(l));
        }
        let message = claude::user_message(text, attachments, &self.sent);
        self.write(&message)
    }

    /// Sends a message into the running turn instead of after it. Claude folds a message sent mid-turn into that
    /// turn already; Codex gets `turn/steer` (or a new turn when none runs).
    pub fn steer(&mut self, text: &str, attachments: &[Attachment]) -> Result<()> {
        attach::check_count(attachments)?;
        if self.cursor.is_some() {
            bail!("Cursor can't steer a running turn; wait for it to finish or stop it");
        }
        if let Some(state) = &self.codex {
            let lines = lock(state).prompt(text, attachments, true)?;
            return lines.into_iter().try_for_each(|l| self.queue(l));
        }
        self.send_message(text, attachments)
    }

    /// Switches the model for the next turns. Claude takes `set_model` (its effort is fixed at launch, so `effort`
    /// applies from the next start); Codex sends both with every turn; Cursor applies it on the next turn.
    pub fn set_model(&mut self, model: Option<&str>, effort: Option<&str>) -> Result<()> {
        for v in [model, effort].into_iter().flatten() {
            if !models::valid_name(v) {
                bail!("refusing model {v:?}");
            }
        }
        if self.cursor.is_some() {
            return Ok(());
        }
        if let Some(state) = &self.codex {
            lock(state).pick = codex::Pick {
                model: model.map(str::to_owned),
                effort: effort.map(str::to_owned),
            };
            return Ok(());
        }
        self.next_request += 1;
        let request_id = format!("skyelab-{}", self.next_request);
        self.write(&json!({
            "type": "control_request",
            "request_id": request_id,
            "request": { "subtype": "set_model", "model": model },
        }))
    }
}
