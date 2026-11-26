//! "Allow for this thread" for Codex. Codex's own `acceptForSession` covers only that exact command, so SkyeLab keeps
//! the thread's rules here, in `skyelab_core::project_rules`' grammar (the exact command, or a word prefix of one
//! plain command, never a shell or interpreter), and answers a later shell ask they cover itself. They last while
//! this agent process runs, like Claude's own thread allows.

use crate::Mode;
use serde_json::Value;
use skyelab_core::project_rules::{self as rules, Kind, Rule};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Codex's command tool name, as its asks arrive.
pub(super) const SHELL: &str = "shell";

/// Command ask fields that only identify or describe it, left out of its input.
pub(super) fn bookkeeping(key: &str, value: &Value) -> bool {
    matches!(
        key,
        "threadId"
            | "turnId"
            | "itemId"
            | "approvalId"
            | "startedAtMs"
            | "commandActions"
            | "proposedExecpolicyAmendment"
            // The answers Codex takes (accept, the execpolicy amendment above, cancel); a network ask still
            // carries its own fields, which aren't bookkeeping.
            | "availableDecisions"
    ) || (key == "kind" && value == "command")
        || (key == "environmentId" && value == "local")
}

pub(super) struct ThreadAllow {
    /// The thread's folder: a rule covers asks whose `cwd` is in it.
    dir: PathBuf,
    /// The mode the running turn has and the one picked for the next: in Plan, no rule answers (an ask there wants out
    /// of the read-only sandbox).
    pub(super) running: Mode,
    pub(super) picked: Mode,
    rules: Vec<Rule>,
    /// Each waiting shell ask's input, by our ask id.
    waiting: BTreeMap<String, Value>,
}

impl ThreadAllow {
    pub(super) fn new(dir: &std::path::Path, mode: Mode) -> Self {
        Self {
            dir: dir.to_owned(),
            running: mode,
            picked: mode,
            rules: Vec::new(),
            waiting: BTreeMap::new(),
        }
    }

    /// The rule that answers this shell ask now, if one does.
    pub(super) fn covers(&self, input: &Value) -> Option<String> {
        if self.planning() {
            return None;
        }
        rules::first_match(&self.rules, SHELL, input, &self.dir).map(rules::label)
    }

    /// Plan runs or is picked: an ask then wants out of the read-only sandbox.
    fn planning(&self) -> bool {
        self.running == Mode::Plan || self.picked == Mode::Plan
    }

    pub(super) fn wait(&mut self, id: String, input: Value) {
        self.waiting.insert(id, input);
    }

    pub(super) fn forget(&mut self, id: &str) {
        self.waiting.remove(id);
    }

    pub(super) fn clear_waiting(&mut self) {
        self.waiting.clear();
    }

    /// "Allow for this thread" on ask `id`: when it is a shell command a rule can cover, that exact command from now on.
    pub(super) fn allow_exact(&mut self, id: &str) {
        if self.planning() {
            return;
        }
        let rule = self
            .waiting
            .get(id)
            .and_then(|input| rules::propose(SHELL, input, &self.dir));
        if let Some(rule) = rule.filter(|r| !self.rules.contains(r)) {
            self.rules.push(rule);
        }
    }

    /// Commands starting with `prefix` from now on, which must be a valid prefix rule covering waiting ask `id`.
    /// Its label, or why not.
    pub(super) fn allow_prefix(&mut self, id: &str, prefix: &str) -> Result<String, String> {
        if self.planning() {
            return Err(
                "In Plan mode a command asks to leave the read-only sandbox: allow it once.".into(),
            );
        }
        let input = self
            .waiting
            .get(id)
            .ok_or("Only a waiting shell command can be allowed by its start.")?;
        let rule = Rule {
            tool: SHELL.to_owned(),
            kind: Kind::Prefix,
            pattern: prefix.trim().to_owned(),
        };
        rules::validate(&rule)?;
        if !rules::matches(&rule, SHELL, input, &self.dir) {
            return Err("These words don't start the command waiting now.".into());
        }
        let label = rules::label(&rule);
        if !self.rules.contains(&rule) {
            self.rules.push(rule);
        }
        Ok(label)
    }
}
