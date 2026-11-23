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
    pub(in crate::workspace) fn rebound(&self, now: &Account) -> Option<SharedString> {
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
    pub(in crate::workspace) fn send_account(&self) -> Result<Account, SharedString> {
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
    pub(in crate::workspace) fn take_attachments(&mut self, external: bool) -> Vec<Attachment> {
        if external {
            Vec::new()
        } else {
            self.composer.pasted = None;
            std::mem::take(&mut self.composer.attachments)
        }
    }

    /// An interrupt was sent (the composer's Interrupt, ⌘. or the palette): "Stopping…" shows until the turn ends, or
    /// for [`INTERRUPT_WAIT`], after which Interrupt works again and Force stop shows.
    pub(in crate::workspace) fn interrupt_pending(&mut self, cx: &mut Context<Self>) {
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
    pub(in crate::workspace) fn after_turn(&mut self, cx: &mut Context<Self>) {
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

    pub(in crate::workspace) fn turn_running(&self) -> bool {
        self.session.is_some() && self.turns.load(std::sync::atomic::Ordering::SeqCst) > 0
    }

    /// What a send of `text` does now.
    pub(in crate::workspace) fn send_mode(&self, how: Submit, text: &str) -> SendMode {
        match how {
            Submit::External => SendMode::Now,
            _ => composer_queue::mode(self.turn_running(), how == Submit::Steer, text),
        }
    }

    /// Sends queued message `id` into the running turn now; with no turn running, sends it as a new one. With no agent
    /// at all (it ended), the message goes back into an empty composer, where Send resumes the session.
    pub(in crate::workspace) fn steer_queued(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

    pub(in crate::workspace) fn remove_queued(&mut self, id: u64, cx: &mut Context<Self>) {
        self.composer.queue.take(id);
        cx.notify();
    }

    /// Puts a message the agent refused back into the composer, once it is empty (the user may be typing anew).
    pub(in crate::workspace) fn restore_draft(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
    pub(in crate::workspace) fn queue_prompt(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let attachments = std::mem::take(&mut self.composer.attachments);
        self.composer.queue.push(text, attachments);
        self.prompt.update(cx, |p, cx| p.set_value("", window, cx));
        self.restore_draft(window, cx); // set_value emits no Change
        // The queue takes room under the messages: keep the end of the thread in view above it.
        self.own_list().scroll_to_end();
        cx.notify();
    }

    /// Sends the prompt (and its attachments, unless `how` is external): starts or resumes the agent, or follows up
    /// on a running one, where a send during a turn queues and a steer goes into it.
    pub(in crate::workspace) fn submit(
        &mut self,
        how: super::composer_state::Submit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use super::composer_queue::SendMode;
        let external = how == super::composer_state::Submit::External;
        let text = self.prompt.read(cx).value().trim().to_string();
        let has_files = !external
            && !(self.composer.attachments.is_empty() && self.composer.contexts.is_empty());
        if (text.is_empty() && !has_files) || self.read_only || self.starting {
            return;
        }
        // `/side <question>` (or `/btw`) asks it in a side chat instead (not from a side chat itself).
        if let Some(question) = crate::workspace::side_chat::side_question(&text)
            && !external
            && !self.is_side_chat()
        {
            if let Some(why) = self.side_refusal() {
                self.composer.notice = Some(why.into()); // the question stays in the composer
                return cx.notify();
            }
            self.prompt.update(cx, |p, cx| p.set_value("", window, cx));
            self.restore_draft(window, cx); // set_value emits no Change
            use crate::workspace::side_chat::SideAsk;
            return cx.emit(crate::workspace::thread::ThreadViewEvent::Side(
                SideAsk::New(question),
            ));
        }
        // Over the provider's limit: refused here, with the text kept, rather than by the agent mid-turn.
        if let Some(why) = skyelab_core::prompt::over_limit(&self.with_contexts(&text, external)) {
            self.composer.notice = Some(why.into());
            cx.notify();
            return;
        }
        // The folder decides the account. Unbound means ask the user, never pick one.
        let resolved = self.config.resolve(&self.cwd).cloned();
        if let (Some(want), Some(got)) = (&self.required_account, &resolved)
            && got.id != *want
        {
            self.error =
                Some(format!("This folder is now bound to {}, not {want}.", got.id).into());
            cx.notify();
            return;
        }
        // Files the account's agent can't take stay in the composer, before anything is sent or cleared.
        if !external
            && let Some(why) = resolved
                .as_ref()
                .and_then(|a| super::composer_state::unfit(a, &self.composer.attachments))
        {
            self.composer.notice = Some(why.into());
            cx.notify();
            return;
        }
        self.composer.menu = None;
        if self.session.is_none()
            && let Some(thread) = self.thread.clone()
        {
            let attachments = self.take_attachments(external);
            let contexts = self.composer.contexts.clone();
            let text = self.take_contexts(text, external);
            self.resume(text, attachments, thread, resolved, window, cx);
            if self.resume_refused.is_some() && !external {
                self.composer.contexts = contexts; // kept with the unsent text
            }
            return;
        }
        let Some(account) = resolved else {
            self.needs_binding = true;
            cx.notify();
            return;
        };
        self.needs_binding = false;
        self.error = None;
        if self.session.is_some() {
            // Checked before anything is queued or taken: a rebound folder needs a new thread.
            if let Some(why) = self.rebound(&account) {
                self.error = Some(why);
                cx.notify();
                return;
            }
            let mode = self.send_mode(how, &text);
            let contexts = self.composer.contexts.clone();
            let text = self.take_contexts(text, external);
            if mode == SendMode::Queue {
                return self.queue_prompt(text, window, cx);
            }
            let attachments = self.take_attachments(external);
            if self.follow_up(text, attachments, mode == SendMode::Steer, &account, cx) {
                self.prompt.update(cx, |p, cx| p.set_value("", window, cx));
                self.restore_draft(window, cx); // set_value emits no Change
            } else if !external {
                self.composer.contexts = contexts; // refused: the prompt stays, so do they
            }
            return;
        }
        let attachments = self.take_attachments(external);
        let text = self.take_contexts(text, external);
        self.launch(text, attachments, account, None, window, cx);
    }

    /// A message from another device or an agent: sent now, as text only, never queued behind a turn and never with
    /// this tab's own attachments.
    pub(in crate::workspace) fn submit_external(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit(Submit::External, window, cx);
    }
}
