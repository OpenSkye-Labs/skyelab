//! A running session's controls: its events, permission mode, interrupt, stderr tail and stdin queue.

use crate::{Event, Mode, Session, launch, lock, lock_acp, lock_cursor};
use anyhow::{Result, bail};
use serde_json::json;

impl Session {
    /// Events in the order `claude` printed them.
    ///
    /// The channel closes when the last holder of claude's stdout closes it, which can be a tool
    /// `claude` started that outlives it, not when the process exits. So a turn is over on an
    /// [`Event::Result`], and the session is over once [`kill`](Self::kill) returns or
    /// [`try_wait`](Self::try_wait) is `Some`.
    pub fn events(&self) -> &flume::Receiver<Event> {
        &self.events
    }

    /// Changes the running agent's permission mode: Claude's and ACP's at once, Codex's from its next turn. Claude
    /// and Antigravity take Full access only if started in it; otherwise this fails and it keeps its mode until it
    /// starts again.
    pub fn set_mode(&mut self, mode: Mode) -> Result<()> {
        if self.cursor.is_some() {
            // Cursor runs one turn per process; the saved mode applies on the next turn.
            return Ok(());
        }
        if let Some(state) = &self.codex {
            lock(state).set_mode(mode);
            return Ok(());
        }
        if mode == Mode::Bypass && self.launched != Mode::Bypass {
            bail!(
                "Full access applies when this agent starts again: stop it, then send a message to continue"
            );
        }
        if let Some(state) = &self.acp {
            let lines = lock_acp(state).set_mode(mode);
            return lines.into_iter().try_for_each(|l| self.queue(l));
        }
        self.next_request += 1;
        let request_id = format!("skyelab-{}", self.next_request);
        self.write(&json!({
            "type": "control_request",
            "request_id": request_id,
            "request": { "subtype": "set_permission_mode", "mode": mode.claude() },
        }))
    }

    /// What the agent wrote to stderr since the last call (the newest [`STDERR_TAIL`] bytes of it).
    pub fn take_stderr(&self) -> String {
        let bytes = std::mem::take(&mut *self.stderr.lock().unwrap_or_else(|e| e.into_inner()));
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// Asks the agent to stop the current turn; the process stays alive for the next message.
    pub fn interrupt(&mut self) -> Result<()> {
        if let Some(state) = &self.cursor {
            lock_cursor(state).interrupted = true;
            if self.status.is_none() {
                #[cfg(unix)]
                launch::interrupt_group(self.child.id());
                #[cfg(not(unix))]
                {
                    self.kill_group();
                    let _ = self.child.kill();
                }
            }
            return Ok(());
        }
        if let Some(state) = &self.codex {
            let lines = lock(state).interrupt();
            return lines.into_iter().try_for_each(|l| self.queue(l));
        }
        if let Some(state) = &self.acp {
            let lines = lock_acp(state).interrupt();
            return lines.into_iter().try_for_each(|l| self.queue(l));
        }
        self.next_request += 1;
        let request_id = format!("skyelab-{}", self.next_request);
        self.write(&json!({
            "type": "control_request",
            "request_id": request_id,
            "request": { "subtype": "interrupt" },
        }))
    }

    pub(crate) fn write(&mut self, msg: &serde_json::Value) -> Result<()> {
        self.queue(msg.to_string())
    }

    /// Queues one line for the writer thread; never blocks.
    pub(crate) fn queue(&self, line: String) -> Result<()> {
        self.stdin.try_send(line).map_err(|e| match e {
            flume::TrySendError::Full(_) => {
                anyhow::anyhow!("the agent isn't reading its input; stop it and try again")
            }
            flume::TrySendError::Disconnected(_) => anyhow::anyhow!("the agent's input is closed"),
        })
    }
}
