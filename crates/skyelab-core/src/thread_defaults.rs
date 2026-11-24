//! A project's own defaults for new threads, layered over Settings' app-wide ones. The project's account is never one
//! of them: that is the folder's binding in accounts.toml.

use crate::{Provider, ThreadSettings};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// One project's overrides, saved under `[threads.projects."<folder>"]`. `None` follows the app-wide default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThreadDefaults {
    pub permission_mode: Option<String>,
    pub claude_model: Option<String>,
    pub claude_effort: Option<String>,
    pub codex_model: Option<String>,
    pub codex_effort: Option<String>,
    pub new_worktree: Option<bool>,
}

/// The text defaults both layers share, so one Settings row edits either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadField {
    PermissionMode,
    ClaudeModel,
    ClaudeEffort,
    CodexModel,
    CodexEffort,
}

/// A model and its effort; `None` leaves it to the next layer (or, at the bottom, the agent's own default).
pub type ModelPair = (Option<String>, Option<String>);

impl ThreadDefaults {
    pub fn field(&self, f: ThreadField) -> &Option<String> {
        match f {
            ThreadField::PermissionMode => &self.permission_mode,
            ThreadField::ClaudeModel => &self.claude_model,
            ThreadField::ClaudeEffort => &self.claude_effort,
            ThreadField::CodexModel => &self.codex_model,
            ThreadField::CodexEffort => &self.codex_effort,
        }
    }

    fn field_mut(&mut self, f: ThreadField) -> &mut Option<String> {
        match f {
            ThreadField::PermissionMode => &mut self.permission_mode,
            ThreadField::ClaudeModel => &mut self.claude_model,
            ThreadField::ClaudeEffort => &mut self.claude_effort,
            ThreadField::CodexModel => &mut self.codex_model,
            ThreadField::CodexEffort => &mut self.codex_effort,
        }
    }
}

impl ThreadSettings {
    pub fn field(&self, f: ThreadField) -> &Option<String> {
        match f {
            ThreadField::PermissionMode => &self.permission_mode,
            ThreadField::ClaudeModel => &self.claude_model,
            ThreadField::ClaudeEffort => &self.claude_effort,
            ThreadField::CodexModel => &self.codex_model,
            ThreadField::CodexEffort => &self.codex_effort,
        }
    }

    pub fn set_field(&mut self, f: ThreadField, value: Option<String>) {
        *match f {
            ThreadField::PermissionMode => &mut self.permission_mode,
            ThreadField::ClaudeModel => &mut self.claude_model,
            ThreadField::ClaudeEffort => &mut self.claude_effort,
            ThreadField::CodexModel => &mut self.codex_model,
            ThreadField::CodexEffort => &mut self.codex_effort,
        } = value;
    }

    /// Sets (or, with `None`, inherits) one of `dir`'s overrides; a project left with none is dropped.
    pub fn set_project_field(&mut self, dir: &Path, f: ThreadField, value: Option<String>) {
        self.edit_project(dir, |p| *p.field_mut(f) = value);
    }

    pub fn set_project_worktree(&mut self, dir: &Path, value: Option<bool>) {
        self.edit_project(dir, |p| p.new_worktree = value);
    }

    fn edit_project(&mut self, dir: &Path, change: impl FnOnce(&mut ThreadDefaults)) {
        let entry = self.projects.entry(dir.to_path_buf()).or_default();
        change(entry);
        if *entry == ThreadDefaults::default() {
            self.projects.remove(dir);
        }
    }

    /// What a new thread in project folder `dir` starts with: the app-wide defaults with that project's set fields on
    /// top. The folder matches whole, never as a prefix.
    pub fn for_project(&self, dir: &Path) -> ThreadSettings {
        let mut out = self.clone();
        if let Some(p) = self.projects.get(dir) {
            let pick = |own: &Option<String>, app: &Option<String>| own.clone().or(app.clone());
            out.permission_mode = pick(&p.permission_mode, &self.permission_mode);
            out.claude_model = pick(&p.claude_model, &self.claude_model);
            out.claude_effort = pick(&p.claude_effort, &self.claude_effort);
            out.codex_model = pick(&p.codex_model, &self.codex_model);
            out.codex_effort = pick(&p.codex_effort, &self.codex_effort);
            out.new_worktree = p.new_worktree.unwrap_or(self.new_worktree);
        }
        out
    }

    /// The model and effort a `provider` thread in `dir` runs with: the thread's own pick, else the project's default
    /// (a half it leaves unset comes from the app's), else the project's remembered last pick, else the app default.
    pub fn resolve_model(
        &self,
        dir: &Path,
        provider: Provider,
        thread_pick: ModelPair,
        last_pick: ModelPair,
    ) -> ModelPair {
        let set = |p: &ModelPair| p.0.is_some() || p.1.is_some();
        if set(&thread_pick) {
            return thread_pick;
        }
        let project = self.projects.get(dir).map(|p| match provider {
            Provider::Claude => (p.claude_model.clone(), p.claude_effort.clone()),
            Provider::Codex => (p.codex_model.clone(), p.codex_effort.clone()),
        });
        if project.as_ref().is_some_and(set) {
            return self.for_project(dir).model_for(provider);
        }
        if set(&last_pick) {
            return last_pick;
        }
        self.model_for(provider)
    }
}
