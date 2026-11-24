//! Resuming an ended thread's agent session, only under the account on its row.

use super::ThreadView;
use super::launch::BUSY_ELSEWHERE;
use crate::workspace::lock;
use gpui::{Context, Window};
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
        match resolved {
            Some(account) if account.id == thread.account_id => {
                self.resume_refused = None;
                self.launch(text, attachments, account, Some(thread), window, cx);
            }
            Some(other) => {
                self.composer.attachments.extend(attachments); // kept with the unsent text
                self.resume_refused = Some(
                    format!(
                        "This folder is now bound to {} ({}), but this thread ran under {}. \
                         Its session can't continue under another account.",
                        other.label, other.id, thread.account_id
                    )
                    .into(),
                );
                cx.notify();
            }
            None => {
                self.composer.attachments.extend(attachments);
                // No path: this text also goes to remote clients (submit_remote).
                self.resume_refused = Some(
                    format!(
                        "This folder isn't bound to {} any more (unbound, or its binding needs \
                         re-confirming in Settings), so this thread can't continue.",
                        thread.account_id
                    )
                    .into(),
                );
                cx.notify();
            }
        }
    }
}
