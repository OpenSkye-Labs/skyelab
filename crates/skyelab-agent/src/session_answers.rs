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
        // One lock for all of it: see `settle`.
        let mut codex = lock(state);
        let (line, rule) = codex.allow_prefix(id, prefix).map_err(anyhow::Error::msg)?;
        self.queue(line)?;
        codex.forget(id);
        self.report(id, Decision::AllowThread, Some(rule));
        Ok(())
    }

    fn answer_noting(&mut self, id: &str, decision: Decision, rule: Option<String>) -> Result<()> {
        if decision == Decision::Withdrawn {
            bail!("an answer allows or denies");
        }
        self.settle(
            id,
            (decision, rule),
            |c| c.answer(id, decision),
            |a| a.answer(id, decision),
            |b| claude::answer(b, id, decision),
            "the agent is no longer waiting for that answer",
        )
    }

    /// Answers the AskUserQuestion ask `id` with each question's picked labels, in question order (a typed "Other"
    /// answer is a label too). Then the session's events carry [`Event::PermissionResolved`] (allowed once). Fails,
    /// sending nothing, when `id` isn't a waiting question or the count doesn't match its questions.
    pub fn answer_questions(&mut self, id: &str, answers: &[Vec<String>]) -> Result<()> {
        self.settle(
            id,
            (Decision::AllowOnce, None),
            |c| c.answer_questions(id, answers),
            |a| a.answer_questions(id, answers),
            |b| claude::answer_questions(b, id, answers),
            "the agent is no longer waiting for these answers",
        )
    }

    /// Queues the reply to open ask `id` (built by `codex`, `acp`, or `claude`), forgets the ask, and reports it
    /// resolved with the decision and the rule that made it, all under the open asks' lock. The reader takes that lock
    /// to withdraw every open ask when a turn ends, and the agent often ends the turn as soon as it reads the reply:
    /// settled any later, the ask would be withdrawn first and its row would never show what was decided.
    /// Fails, sending nothing, with `gone` when no reply can be built (the ask is no longer open). Forgotten only once
    /// queued: a full queue leaves it open, to be answered again.
    fn settle(
        &self,
        id: &str,
        (decision, rule): (Decision, Option<String>),
        codex: impl FnOnce(&mut codex::Codex) -> Option<String>,
        acp: impl FnOnce(&acp::Acp) -> Option<String>,
        claude: impl FnOnce(&claude::Book) -> Option<Value>,
        gone: &'static str,
    ) -> Result<()> {
        if self.cursor.is_some() {
            bail!("{gone}");
        }
        if let Some(state) = &self.codex {
            let mut c = lock(state);
            self.queue(codex(&mut c).context(gone)?)?;
            c.forget(id);
            self.report(id, decision, rule);
        } else if let Some(state) = &self.acp {
            let mut a = lock_acp(state);
            self.queue(acp(&a).context(gone)?)?;
            a.forget(id);
            self.report(id, decision, rule);
        } else {
            let mut book = claude::asks(&self.asks);
            self.queue(claude(&book).context(gone)?.to_string())?;
            claude::forget(&mut book, id);
            self.report(id, decision, rule);
        }
        Ok(())
    }

    /// Tells the session's events that ask `id` was answered.
    fn report(&self, id: &str, decision: Decision, rule: Option<String>) {
        if let Some(tx) = self.events_tx.upgrade() {
            let _ = tx.send(Event::PermissionResolved {
                id: id.to_owned(),
                decision,
                rule,
            });
        }
    }
}
