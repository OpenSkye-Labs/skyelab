//! Starting `claude` for a thread: a new thread, a follow-up, or resuming an ended session.

use super::{Record, ThreadView, ThreadViewEvent, append};
use crate::workspace::lock;
use gpui::{AppContext as _, Context, Window};
use skyelab_agent::Launch;
use skyelab_core::Account;
use skyelab_store::{NewThread, Thread};

/// The first line of the prompt, cut to a tab-sized title.
fn title(prompt: &str) -> String {
    let line = prompt.lines().next().unwrap_or_default().trim();
    let mut t: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        t.push('…');
    }
    t
}

impl ThreadView {
    /// Continues an ended thread's `claude` session, only under the account on its row: the
    /// session's files live in that account's config dir.
    pub(super) fn resume(
        &mut self,
        text: String,
        thread: Thread,
        resolved: Option<Account>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.error = None;
        match resolved {
            Some(account) if account.id == thread.account_id => {
                self.resume_refused = None;
                self.launch(text, account, Some(thread), window, cx);
            }
            Some(other) => {
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
                self.resume_refused = Some(
                    format!(
                        "{} isn't bound to {} any more (unbound, or its binding needs \
                         re-confirming in Settings), so this thread can't continue.",
                        self.cwd.display(),
                        thread.account_id
                    )
                    .into(),
                );
                cx.notify();
            }
        }
    }

    /// Starts `claude` under `account`: a new thread row, or `resume`'s session.
    pub(super) fn launch(
        &mut self,
        text: String,
        account: Account,
        resume: Option<Thread>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.starting = true;
        self.stop_requested = false;
        self.launches += 1;
        self.prompt.update(cx, |p, cx| p.set_value("", window, cx));
        self.push(&Record::Prompt(text.clone()), cx);

        let (store, bin, config, cwd, project_id) = (
            self.backend.store.clone(),
            self.backend.claude_bin.clone(),
            self.config.clone(),
            self.cwd.clone(),
            self.project_id,
        );
        let is_new = resume.is_none();
        let pump_store = store.clone();
        self._task = Some(cx.spawn(async move |this, cx| {
            let started = cx
                .background_spawn(async move {
                    let thread = match resume {
                        Some(thread) => thread,
                        None => lock(&store)?.create_thread(&NewThread {
                            project_id,
                            title: &title(&text),
                            cwd: &cwd,
                            account_id: &account.id,
                            status: "starting",
                        })?,
                    };
                    append(
                        &mut *lock(&store)?,
                        thread.id,
                        &Record::Prompt(text.clone()),
                    )?;
                    // skyelab-agent re-checks `account` against the folder at spawn and aborts if it moved.
                    let launch = Launch {
                        account: &account,
                        dir: &cwd,
                        resume: thread.agent_session_id.as_deref(), // a new row has none
                        prompt: &text,
                    };
                    let session = skyelab_agent::start_with_bin(&bin, &config, &launch);
                    let status = if session.is_ok() { "running" } else { "failed" };
                    lock(&store)?.set_thread_status(thread.id, status)?;
                    anyhow::Ok((thread, session))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.starting = false;
                match started {
                    Ok((thread, session)) => {
                        this.thread = Some(thread.clone());
                        if is_new {
                            cx.emit(ThreadViewEvent::Started(thread.clone()));
                        } else {
                            this.read_only = false;
                            this.prompt.update(cx, |p, cx| p.set_disabled(false, cx));
                        }
                        match session {
                            Ok(session) => {
                                this.run(session, pump_store, thread.id, cx);
                                if this.stop_requested {
                                    this.kill(cx);
                                }
                            }
                            Err(e) => {
                                this.finish(cx);
                                this.show_error(e.context("starting the agent"), cx);
                            }
                        }
                    }
                    Err(e) => this.show_error(e, cx),
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn follow_up(
        &mut self,
        text: String,
        account: &Account,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(session), Some(thread)) = (self.session.clone(), &self.thread) else {
            return;
        };
        // The running agent keeps the account it started with; a rebound folder needs a new thread.
        if thread.account_id != account.id {
            let msg = format!(
                "This folder is now bound to {}. Start a new thread to run under it.",
                account.label
            );
            self.error = Some(msg.into());
            cx.notify();
            return;
        }
        let (store, id) = (self.backend.store.clone(), thread.id);
        self.prompt.update(cx, |p, cx| p.set_value("", window, cx));
        let record = Record::Prompt(text.clone());
        self.push(&record, cx);
        // Chained, so quick follow-ups reach the store and claude in the order they were sent.
        let prev = self.send_chain.take();
        let write = cx.background_executor().clone();
        self.send_chain = Some(cx.spawn(async move |this, cx| {
            if let Some(prev) = prev {
                prev.await;
            }
            let sent = write
                .spawn(async move {
                    // Queued first, so a full queue stores no prompt claude never got.
                    lock(&session)?.send_user_message(&text)?;
                    append(&mut *lock(&store)?, id, &record)?;
                    lock(&store)?.set_thread_status(id, "running")
                })
                .await;
            if let Err(e) = sent {
                let _ = this.update(cx, |this, cx| this.show_error(e, cx));
            }
        }));
    }
}
