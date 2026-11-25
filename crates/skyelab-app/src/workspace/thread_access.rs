//! The thread view's small setters and getters: accounts, PR link, focus, title, folded tool rows.

use super::ThreadView;
use gpui::{App, Context, Focusable as _, SharedString, Window};
use skyelab_core::AccountsConfig;

impl ThreadView {
    /// Accounts changed on disk (Settings saved). Used for the next start and the badge.
    pub fn set_config(&mut self, config: AccountsConfig, cx: &mut Context<Self>) {
        self.config = config;
        let old = self.composer_account().map(|a| a.id);
        self.folder_account = self.config.resolve(&self.cwd).cloned();
        if self.composer_account().map(|a| a.id) != old {
            // Another account now: its own default model and provider model lists.
            (
                self.composer.account_model,
                self.composer.codex_models,
                self.composer.cursor_models,
                self.composer.antigravity_models,
            ) = (None, None, None, None);
            self.load_account_model(cx);
        }
        self.resume_refused = None; // the next send checks the new bindings again
        self.drop_unbound_chips();
        if self.needs_binding && self.config.resolve(&self.cwd).is_some() {
            self.needs_binding = false;
        }
        cx.notify();
    }

    /// A PR was opened from this thread (already stored on its row).
    pub fn set_pr_url(&mut self, url: Option<String>, cx: &mut Context<Self>) {
        if let Some(t) = self.thread.as_mut() {
            t.pr_url = url;
            t.pr_state = None; // the store clears it with the link
            cx.notify();
        }
    }

    pub fn focus_prompt(&self, window: &mut Window, cx: &mut App) {
        self.prompt.read(cx).focus_handle(cx).focus(window, cx);
    }

    pub fn title(&self) -> SharedString {
        match &self.thread {
            Some(t) => t.title.clone().into(),
            None => "New thread".into(),
        }
    }

    /// What the sidebar shows for this thread: its live session's state, or with no session, stopped (the stored row
    /// may still say running from before).
    pub fn is_expanded(&self, tool_id: &str) -> bool {
        self.expanded.contains(tool_id)
    }

    pub fn toggle_tool(&mut self, tool_id: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(tool_id) {
            self.expanded.insert(tool_id.to_owned());
        }
        if let Some(rows) = crate::workspace::timeline_fold::rows_of(&self.items, tool_id) {
            let n = rows.len();
            self.lists.splice(rows, n);
        }
        cx.notify();
    }
}
