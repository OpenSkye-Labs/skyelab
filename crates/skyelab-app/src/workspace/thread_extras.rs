//! What a thread has beyond running its agent: the defaults new threads start with (Settings → Threads), generated
//! titles and forks. Drafts and prompt history are the composer's (`composer_draft.rs`).

#[path = "fork.rs"]
pub(super) mod fork;
#[path = "thread_prefs.rs"]
pub(crate) mod prefs;
#[path = "thread_title.rs"]
pub(super) mod title;

#[cfg(all(test, unix))]
#[path = "draft_test.rs"]
mod draft_test;
#[cfg(test)]
#[path = "fork_cut_gone_test.rs"]
mod fork_cut_gone_test;
#[cfg(all(test, unix))]
#[path = "fork_test.rs"]
mod fork_test;
#[cfg(all(test, unix))]
#[path = "prompt_history_test.rs"]
mod prompt_history_test;
#[cfg(all(test, unix))]
#[path = "thread_defaults_test.rs"]
mod thread_defaults_test;
#[cfg(all(test, unix))]
#[path = "thread_title_off_test.rs"]
mod thread_title_off_test;
#[cfg(all(test, unix))]
#[path = "thread_title_regen_test.rs"]
mod thread_title_regen_test;
#[cfg(all(test, unix))] // the fake agents are shell scripts
#[path = "thread_title_test.rs"]
mod thread_title_test;

use super::thread::ThreadView;

impl ThreadView {
    /// "Label · id" of the account the thread was started with, from its store row.
    #[cfg(all(test, unix))] // only the unix-only thread tests read it
    pub fn badge(&self) -> Option<gpui::SharedString> {
        let id = &self.thread.as_ref()?.account_id;
        let label = self
            .config
            .accounts
            .iter()
            .find(|a| &a.id == id)
            .map_or("Unknown account", |a| a.label.as_str());
        Some(format!("{label} · {id}").into())
    }

    /// The provider of the account the thread was started with (`None` before the thread has a row,
    /// or when that account is gone).
    #[cfg(all(test, unix))]
    pub fn provider(&self) -> Option<skyelab_core::Provider> {
        let id = &self.thread.as_ref()?.account_id;
        let account = self.config.accounts.iter().find(|a| &a.id == id)?;
        Some(account.provider)
    }
}
