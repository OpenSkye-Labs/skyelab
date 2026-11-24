//! Resuming an ended thread's agent session, only under the account on its row.

use super::ThreadView;
use super::launch::BUSY_ELSEWHERE;
use crate::workspace::lock;
use gpui::{Context, SharedString, Window};
use skyelab_core::Account;
use skyelab_store::Thread;

impl ThreadView {
    /// Continues an ended thread's `claude` session, only under the account on its row: the
    /// session's files live in that account's config dir.
    pub(in crate::workspace) fn resume(
        &mut self,
        text: String,
        attachments: Vec<skyelab_agent::Attachment>,
        thread: Thread,
        resolved: Option<Account>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.error = None;
        if let Some(session) = thread.agent_session_id.as_deref()
            && lock(&self.backend.store)
                .and_then(|s| s.session_busy_elsewhere(thread.id, session))
                .unwrap_or(false)
        {
            self.composer.attachments.extend(attachments); // kept with the unsent text
            self.resume_refused = Some(BUSY_ELSEWHERE.into());
            return cx.notify();
        }
        match refusal(&thread.account_id, resolved.as_ref()) {
            None => {
                self.resume_refused = None;
                if let Some(account) = resolved {
                    self.launch(text, attachments, account, Some(thread), window, cx);
                }
            }
            Some(why) => {
                self.composer.attachments.extend(attachments); // kept with the unsent text
                self.resume_refused = Some(why.into());
                cx.notify();
            }
        }
    }

    /// The folder's binding moved under the running agent (a send re-checked it): the agent ends, and the thread shows
    /// why it can't continue here, as a resume would.
    pub(in crate::workspace) fn end_refused(&mut self, why: SharedString, cx: &mut Context<Self>) {
        self.resume_refused = Some(why);
        self.kill(cx);
        cx.notify();
    }
}

/// Why a thread that ran under `account_id` can't continue under the folder's binding `now` (its session lives in
/// that account's dir); `None` when it can. Never another account. No path: this text also goes to remote clients.
pub(in crate::workspace) fn refusal(account_id: &str, now: Option<&Account>) -> Option<String> {
    match now {
        Some(a) if a.id == account_id => None,
        Some(other) => Some(format!(
            "This folder is now bound to {} ({}), but this thread ran under {account_id}. Its session can't \
             continue under another account.",
            other.label, other.id
        )),
        None => Some(format!(
            "This folder isn't bound to {account_id} any more (unbound, or its binding needs re-confirming in \
             Settings), so this thread can't continue."
        )),
    }
}
