//! Sending from the composer while an agent runs: the account check a queued message needs, the queue and its
//! Steer, giving a refused message back, and Interrupt's "Stopping…" state.

use super::composer_queue::{self, SendMode};
use super::composer_state::Submit;
use super::thread::ThreadView;
use gpui::{Context, SharedString, Window};
use skyelab_agent::Attachment;
use skyelab_core::Account;

/// How long Interrupt waits for the turn to end before it can be pressed again (and Force stop shows), as T3's.
const INTERRUPT_WAIT: std::time::Duration = std::time::Duration::from_secs(10);
impl ThreadView {
    /// Why a send to the running agent must be refused: the folder no longer resolves to the thread's account.
    pub(super) fn rebound(&self, now: &Account) -> Option<SharedString> {
        let thread = self.thread.as_ref()?;
        (thread.account_id != now.id).then(|| {
            format!(
                "This folder is now bound to {}. Start a new thread to run under it.",
                now.label
            )
            .into()
        })
    }

    /// The account a queued message may go to now: the folder's, which must still be the thread's.
    pub(super) fn send_account(&self) -> Result<Account, SharedString> {
        let Some(now) = self.config.resolve(&self.cwd).cloned() else {
            return Err(
                "This folder isn't bound to an account any more; the queued messages wait.".into(),
            );
        };
        if let Some(want) = &self.required_account
            && now.id != *want
        {
            return Err(format!("This folder is now bound to {}, not {want}.", now.id).into());
        }
        match self.rebound(&now) {
            Some(why) => Err(why),
            None => Ok(now),
        }
    }

    /// The composer's files for a send; an outside sender's message never takes them.
    pub(super) fn take_attachments(&mut self, external: bool) -> Vec<Attachment> {
        if external {
            Vec::new()
        } else {
            std::mem::take(&mut self.composer.attachments)
        }
    }

    /// An interrupt was sent (the composer's Interrupt, ⌘. or the palette): "Stopping…" shows until the turn ends, or
    /// for [`INTERRUPT_WAIT`], after which Interrupt works again and Force stop shows.
    pub(super) fn interrupt_pending(&mut self, cx: &mut Context<Self>) {
        let n = self.composer.interrupting.map_or(1, |n| n + 1);
        self.composer.interrupting = Some(n);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(INTERRUPT_WAIT).await;
            let _ = this.update(cx, |this, cx| {
                if this.composer.interrupting == Some(n) && this.turn_running() {
                    this.composer.interrupting = None;
                    this.composer.stalled = true;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// A turn ended: refresh the footer, and when no turn is left, send the next queued message.
    pub(super) fn after_turn(&mut self, cx: &mut Context<Self>) {
        self.refresh_footer(cx);
        if !self.turn_running() {
            self.composer.interrupting = None;
            self.composer.stalled = false;
        }
        if self.turn_running() || self.session.is_none() {
            return;
        }
        if self.composer.queue.is_empty() {
            return;
        }
        let account = match self.send_account() {
            Ok(account) => account,
            Err(why) => {
                self.error = Some(why); // the queue stays as it is
                cx.notify();
                return;
            }
        };
        if let Some(next) = self.composer.queue.pop()
            && !self.follow_up(
                next.text.clone(),
                next.attachments.clone(),
                false,
                &account,
                cx,
            )
        {
            self.composer.queue.put_back(next);
        }
        cx.notify();
    }

    pub(super) fn turn_running(&self) -> bool {
        self.session.is_some() && self.turns.load(std::sync::atomic::Ordering::SeqCst) > 0
    }

    /// What a send of `text` does now.
    pub(super) fn send_mode(&self, how: Submit, text: &str) -> SendMode {
        match how {
            Submit::External => SendMode::Now,
            _ => composer_queue::mode(self.turn_running(), how == Submit::Steer, text),
        }
    }

    /// Sends queued message `id` into the running turn now; with no turn running, sends it as a new one. With no agent
    /// at all (it ended), the message goes back into an empty composer, where Send resumes the session.
    pub(super) fn steer_queued(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.is_none() {
            let draft = !self.prompt.read(cx).value().trim().is_empty()
                || !self.composer.attachments.is_empty();
            if draft {
                self.composer.notice =
                    Some("Send or clear your draft first; the agent isn't running.".into());
            } else if let Some(q) = self.composer.queue.take(id) {
                self.prompt
                    .update(cx, |p, cx| p.set_value(q.text.clone(), window, cx));
                self.composer.attachments = q.attachments;
            }
            cx.notify();
            return;
        }
        let account = match self.send_account() {
            Ok(account) => account,
            Err(why) => {
                self.error = Some(why);
                cx.notify();
                return;
            }
        };
        let Some(text) = self
            .composer
            .queue
            .iter()
            .find(|q| q.id == id)
            .map(|q| q.text.clone())
        else {
            return;
        };
        let mode = composer_queue::mode(self.turn_running(), true, &text);
        if mode == SendMode::Queue {
            self.composer.notice = Some(
                "A command waits for the turn to end; steering it would replace the task.".into(),
            );
            cx.notify();
            return;
        }
        if let Some(q) = self.composer.queue.take(id) {
            self.follow_up(q.text, q.attachments, mode == SendMode::Steer, &account, cx);
        }
        cx.notify();
    }

    pub(super) fn remove_queued(&mut self, id: u64, cx: &mut Context<Self>) {
        self.composer.queue.take(id);
        cx.notify();
    }

    /// Puts a message the agent refused back into the composer, once it is empty (the user may be typing anew).
    pub(super) fn restore_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.composer.restore.is_none()
            || !self.prompt.read(cx).value().is_empty()
            || !self.composer.attachments.is_empty()
        {
            return;
        }
        if let Some((text, attachments)) = self.composer.restore.take() {
            self.prompt
                .update(cx, |p, cx| p.set_value(text, window, cx));
            self.composer.attachments = attachments;
        }
    }

    /// Holds `text` and the attachments for after the running turn, shown in the queue above the composer.
    pub(super) fn queue_prompt(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let attachments = std::mem::take(&mut self.composer.attachments);
        self.composer.queue.push(text, attachments);
        self.prompt.update(cx, |p, cx| p.set_value("", window, cx));
        // The queue takes room under the messages: keep the end of the thread in view above it.
        self.list.scroll_to_end();
        cx.notify();
    }
}
