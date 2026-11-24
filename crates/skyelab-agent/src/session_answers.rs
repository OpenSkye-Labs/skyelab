//! Answering what a running agent asks: permissions, questions, and Codex's thread prefix allows.

use super::*;

impl Session {
    /// Answers the [`Event::Permission`] with this `id`; the agent was waiting for it. Then the session's events
    /// carry [`Event::PermissionResolved`]. Fails, sending nothing, when `id` is no longer waiting (answered,
    /// withdrawn, or never asked).
    pub fn answer(&mut self, id: &str, decision: Decision) -> Result<()> {
        self.answer_noting(id, decision, None)
    }

    /// Allows the [`Event::Permission`] `id` once on behalf of the user, by the project rule `rule` (its label), which
    /// the resolved event carries so the thread says what allowed it.
    pub fn answer_by_rule(&mut self, id: &str, rule: String) -> Result<()> {
        self.answer_noting(id, Decision::AllowOnce, Some(rule))
    }

    /// Allows the Codex shell ask `id`, and every later plain command in this thread that starts with `prefix`, for as
    /// long as this agent runs (`project_rules`' prefix grammar: never a shell, interpreter or compound command).
    /// Then the session's events carry [`Event::PermissionResolved`] naming the rule. Fails, sending nothing, for
    /// Claude (its own thread allow takes its suggested rule), a prefix that isn't valid or doesn't cover the ask, or
    /// an ask no longer waiting.
    pub fn allow_thread_prefix(&mut self, id: &str, prefix: &str) -> Result<()> {
        let Some(state) = &self.codex else {
            bail!("only a Codex thread allows commands by their start");
        };
        let (line, rule) = lock(state)
            .allow_prefix(id, prefix)
            .map_err(anyhow::Error::msg)?;
        self.queue(line)?;
        lock(state).forget(id);
        if let Some(tx) = self.events_tx.upgrade() {
            let _ = tx.send(Event::PermissionResolved {
                id: id.to_owned(),
                decision: Decision::AllowThread,
                rule: Some(rule),
            });
        }
        Ok(())
    }

    fn answer_noting(&mut self, id: &str, decision: Decision, rule: Option<String>) -> Result<()> {
        if decision == Decision::Withdrawn {
            bail!("an answer allows or denies");
        }
        let line = match &self.codex {
            Some(state) => lock(state).answer(id, decision),
            None => claude::answer(&self.asks, id, decision).map(|v| v.to_string()),
        };
        let Some(line) = line else {
            bail!("the agent is no longer waiting for that answer");
        };
        // Forgotten only once queued: a full queue leaves it open, to be answered again.
        self.queue(line)?;
        match &self.codex {
            Some(state) => lock(state).forget(id),
            None => claude::forget(&self.asks, id),
        }
        if let Some(tx) = self.events_tx.upgrade() {
            let _ = tx.send(Event::PermissionResolved {
                id: id.to_owned(),
                decision,
                rule,
            });
        }
        Ok(())
    }

    /// Answers the AskUserQuestion ask `id` with each question's picked labels, in question order (a typed "Other"
    /// answer is a label too). Then the session's events carry [`Event::PermissionResolved`] (allowed once). Fails,
    /// sending nothing, when `id` isn't a waiting question or the count doesn't match its questions.
    pub fn answer_questions(&mut self, id: &str, answers: &[Vec<String>]) -> Result<()> {
        let line = match &self.codex {
            Some(state) => lock(state).answer_questions(id, answers),
            None => claude::answer_questions(&self.asks, id, answers).map(|v| v.to_string()),
        };
        let Some(line) = line else {
            bail!("the agent is no longer waiting for these answers");
        };
        self.queue(line)?;
        match &self.codex {
            Some(state) => lock(state).forget(id),
            None => claude::forget(&self.asks, id),
        }
        if let Some(tx) = self.events_tx.upgrade() {
            let _ = tx.send(Event::PermissionResolved {
                id: id.to_owned(),
                decision: Decision::AllowOnce,
                rule: None,
            });
        }
        Ok(())
    }
}
