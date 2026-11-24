//! Releasing an idle agent (T3's idle session release): an agent left with no turn running for [`IDLE_RELEASE`]
//! is ended, and the next prompt resumes its session, as after a restart. Every thread the user chatted in kept its
//! agent process (and its memory) until SkyeLab quit.
//!
//! Never while it holds work that outlives a turn: a command or subagent it started in the background, a subagent
//! still running, or an approval still asked; nor in a delegation (a parent sends to its child's running agent).
//! Those pin it until its process ends on its own.

use super::{Item, ThreadView, lock};
use anyhow::Context as _;
use gpui::{AppContext as _, Context, Task};
use std::time::Duration;

/// How long an agent may sit idle before it is ended. T3 ends one after 2 minutes; a resume costs the next prompt
/// the agent's start-up, so this waits longer.
pub(super) const IDLE_RELEASE: Duration = Duration::from_secs(10 * 60);

pub(in crate::workspace) struct Release {
    /// How long an idle agent is kept (tests shorten it).
    pub(in crate::workspace) after: Duration,
    /// The first row this agent added: what it started in the background is in the rows from here.
    from: usize,
    /// Ending it is under way: its end is recorded as idle, not as stopped or exited.
    pub(super) releasing: bool,
    _timer: Option<Task<()>>,
}

impl Default for Release {
    fn default() -> Self {
        Self {
            after: IDLE_RELEASE,
            from: 0,
            releasing: false,
            _timer: None,
        }
    }
}

impl Release {
    /// A new agent started; its own rows begin at `from`.
    pub(super) fn started(&mut self, from: usize) {
        self.from = from;
        self.releasing = false;
        self._timer = None;
    }

    /// A prompt went in: the agent is busy again.
    pub(super) fn disarm(&mut self) {
        self._timer = None;
    }
}

/// Work in `rows` (one agent's rows) that would die with its process: a command or subagent started in the
/// background, a subagent still running, an approval not yet answered.
pub(in crate::workspace) fn holds_work(rows: &[Item]) -> bool {
    rows.iter().any(|row| match row {
        Item::Tool(t) => t.raw["run_in_background"] == true,
        Item::Subagent(s) => s.status == skyelab_agent::timeline::subagent::SubagentStatus::Running,
        Item::Approval(a) => a.decision.is_none(),
        _ => false,
    })
}

impl ThreadView {
    /// A turn ended: if the agent is idle now, it is ended once it has stayed idle for [`Release::after`].
    pub(super) fn arm_release(&mut self, cx: &mut Context<Self>) {
        if !self.releasable() {
            return;
        }
        let after = self.release.after;
        self.release._timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(after).await;
            let _ = this.update(cx, |this, cx| this.release_now(cx));
        }));
    }

    fn releasable(&self) -> bool {
        self.session.is_some()
            && !self.starting
            && !self.stop_requested
            && !self.turn_running()
            && !self.release.releasing
            && !self.in_delegation()
            && !holds_work(self.items.get(self.release.from..).unwrap_or_default())
    }

    /// A delegated thread, or one with children still live: the other side sends to its running agent
    /// (`send_to_thread` refuses a thread with none), so it is never released.
    fn in_delegation(&self) -> bool {
        use crate::ui::Status;
        self.thread
            .as_ref()
            .is_some_and(|t| t.parent_thread_id.is_some())
            || self
                .children
                .iter()
                .any(|c| matches!(c.status, Status::Starting | Status::Running | Status::Idle))
    }

    /// Ends the idle agent; the thread stays idle (its row already says so), and the next send resumes its session.
    /// The view lets go of the session at once, so a send that comes while the kill runs takes the resume path.
    fn release_now(&mut self, cx: &mut Context<Self>) {
        self.release._timer = None;
        if !self.releasable() {
            return;
        }
        let Some(session) = self.session.take() else {
            return;
        };
        self.release.releasing = true;
        self.finish(cx);
        let ended =
            cx.background_spawn(
                async move { lock(&session)?.kill().context("ending the idle agent") },
            );
        cx.spawn(async move |this, cx| {
            if let Err(e) = ended.await {
                let _ = this.update(cx, |this, cx| {
                    this.release.releasing = false;
                    this.show_error(e, cx);
                });
            }
        })
        .detach(); // `this` is weak; the kill itself runs to the end either way
    }
}

#[cfg(test)]
#[path = "thread_release_test.rs"]
mod tests;
