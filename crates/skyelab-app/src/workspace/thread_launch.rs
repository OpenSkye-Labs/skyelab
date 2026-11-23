//! Starting `claude` for a thread: a new thread, a follow-up, or resuming an ended session.

use super::{Record, ThreadView, ThreadViewEvent, append};
use crate::workspace::lock;
use anyhow::Context as _;
use gpui::{AppContext as _, Context, Window};
use skyelab_agent::Launch;
use skyelab_core::{Account, Provider};
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

    /// Starts the account's agent (`claude` or `codex`) under `account`: a new thread row, or `resume`'s session.
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

        let bin = match account.provider {
            Provider::Claude => self.backend.claude_bin.clone(),
            Provider::Codex => crate::workspace::github::codex_bin(cx),
        };
        let (store, config, cwd, project_id) = (
            self.backend.store.clone(),
            self.config.clone(),
            self.cwd.clone(),
            self.project_id,
        );
        // A resumed thread keeps the folder it started in (its worktree, if it had one).
        let use_worktree = resume.is_none() && self.wt.use_worktree;
        let parent = self.parent.as_ref().map(|p| p.0);
        let tools = crate::workspace::orchestrate::Tools::get(cx);
        let accounts_file = cx
            .try_global::<crate::workspace::AccountsFile>()
            .map(|f| f.0.clone());
        let is_new = resume.is_none();
        let pump_store = store.clone();
        // The row reaches the view as soon as it is saved, before the agent is up: a delegated start answers then.
        let (saved_tx, saved_rx) = flume::bounded::<Thread>(1);
        let saved_late = saved_rx.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(thread) = saved_rx.recv_async().await {
                // Once the launch has ended, its own result decides what the view keeps.
                let _ = this.update(cx, |this, cx| {
                    if this.starting {
                        this.row_saved(&thread, cx)
                    }
                });
            }
        })
        .detach();
        let unsaved = self.unsaved.clone();
        self._task = Some(cx.spawn(async move |this, cx| {
            let started = cx
                .background_spawn(async move {
                    // The thread whose slot this launch took, so a failure below gives it back.
                    let mut claimed = None;
                    let started = (|| {
                        let worktree = use_worktree
                            .then(|| {
                                crate::workspace::worktree::create(
                                    &config,
                                    &cwd,
                                    &account.id,
                                    &text,
                                )
                            })
                            .transpose()
                            .context("creating the thread's worktree")?;
                        let project = cwd;
                        let cwd = worktree
                            .as_ref()
                            .map_or(project.clone(), |w| w.path.clone());
                        let thread = match resume {
                            Some(thread) => {
                                // A resumed child takes a slot again, under the same lock as its status.
                                if let Some(parent) = thread.parent_thread_id {
                                    let store = lock(&store)?;
                                    crate::workspace::child_slots::claim(
                                        &store,
                                        parent,
                                        Some(thread.id),
                                    )?;
                                    store.set_thread_status(thread.id, "starting")?;
                                    claimed = Some(thread.id);
                                }
                                thread
                            }
                            None => crate::workspace::worktree::save(
                                &store,
                                &project,
                                &NewThread {
                                    project_id,
                                    title: &title(&text),
                                    cwd: &cwd,
                                    account_id: &account.id,
                                    status: "starting",
                                },
                                worktree,
                                parent,
                            )?,
                        };
                        // A new row, or a child's slot: either must not stay busy if this launch fails.
                        if is_new || thread.parent_thread_id.is_some() {
                            claimed = Some(thread.id);
                        }
                        let _ = saved_tx.send(thread.clone());
                        // A resumed thread's held events go first, or its prompt would be stored ahead of them.
                        unsaved
                            .flush(&store, thread.id)
                            .context("earlier events of this thread aren't saved yet")?;
                        append(
                            &mut *lock(&store)?,
                            thread.id,
                            &Record::Prompt(text.clone()),
                        )?;
                        let grant = tools.and_then(|t| t.grant(thread.id, &account.id));
                        // skyelab-agent re-checks `account` against the folder at spawn and aborts if it moved.
                        let launch = Launch {
                            account: &account,
                            dir: &cwd,
                            resume: thread.agent_session_id.as_deref(), // a new row has none
                            prompt: &text,
                            mcp_config: grant.as_ref().map(|g| g.config_path()),
                        };
                        // The binding as saved right now: a rebind may have landed while this launch ran. An
                        // unreadable file binds nothing, so the check below refuses.
                        let current = super::accounts_now(accounts_file.as_deref(), &config);
                        let session = skyelab_agent::start_with_bin(&bin, &current, &launch);
                        let status = if session.is_ok() { "running" } else { "failed" };
                        lock(&store)?.set_thread_status(thread.id, status)?;
                        // From here on, a closed tab can't leave the row running with nothing behind it.
                        let session = session
                            .map(|s| super::session::NewSession::new(s, store.clone(), thread.id));
                        anyhow::Ok((thread, session, grant))
                    })();
                    // A slot is never held by a thread that isn't running.
                    if let (Err(_), Some(id)) = (&started, claimed) {
                        let _ = lock(&store).and_then(|s| s.set_thread_status(id, "failed"));
                    }
                    started
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.starting = false;
                match started {
                    Ok((thread, session, grant)) => {
                        this.row_saved(&thread, cx);
                        this.thread = Some(thread.clone());
                        if !is_new {
                            this.read_only = false;
                            this.prompt.update(cx, |p, cx| p.set_disabled(false, cx));
                        }
                        match session.map(|s| s.take()) {
                            Ok(Some(session)) => {
                                this.grant = grant;
                                this.run(session, pump_store, thread.id, cx);
                                if this.stop_requested {
                                    this.kill(cx);
                                }
                            }
                            Ok(None) => {}
                            Err(e) => {
                                this.finish(cx);
                                this.show_error(e.context("starting the agent"), cx);
                            }
                        }
                    }
                    Err(e) => {
                        // A row saved before the failure belongs to this view even if its hand-off hasn't run.
                        if let Ok(thread) = saved_late.try_recv() {
                            this.row_saved(&thread, cx);
                        }
                        // Nothing runs on it: read-only now, so a remote start's reply doesn't wait for an agent.
                        if this.thread.is_some() {
                            this.finish(cx);
                        }
                        this.show_error(e, cx)
                    }
                }
                cx.notify();
            });
        }));
    }

    /// The launch saved `thread`'s row: the view (and the sidebar, through `Started`) has it from now on, even
    /// while the agent is still starting. A resumed thread already had it.
    fn row_saved(&mut self, thread: &Thread, cx: &mut Context<Self>) {
        if self.thread.as_ref().is_some_and(|t| t.id == thread.id) {
            return;
        }
        self.cwd = thread.cwd.clone();
        self.thread = Some(thread.clone());
        cx.emit(ThreadViewEvent::Started(Box::new(thread.clone())));
        cx.notify();
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
        let (store, id, parent) = (
            self.backend.store.clone(),
            thread.id,
            thread.parent_thread_id,
        );
        let turns = self.turns.clone();
        let on_sent = self.on_sent.take();
        let unsaved = self.unsaved.clone();
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
                    // An idle child that gets a new turn takes a slot again: claimed with its status write.
                    let was = {
                        let store = lock(&store)?;
                        let was = store.thread(id)?.map(|t| t.status).unwrap_or_default();
                        // Stop or an exit got here first: writing "running" would hold a slot with no process.
                        if !matches!(was.as_str(), "idle" | "running" | "starting") {
                            anyhow::bail!("the agent has ended; send again to resume it");
                        }
                        if let Some(parent) = parent
                            && !skyelab_core::delegation::is_busy(&was)
                        {
                            crate::workspace::child_slots::claim(&store, parent, Some(id))?;
                        }
                        store.set_thread_status(id, "running")?;
                        // Counted before it is sent, so a quick result can't find zero and mark the row idle early,
                        // and under the store lock, so a held idle can't land between "running" and the count.
                        turns.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        was
                    };
                    // Queued first, so a full queue stores no prompt claude never got.
                    if let Err(e) = lock(&session).and_then(|mut s| s.send_user_message(&text)) {
                        turns.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                        // Only if nothing (a result, Stop) has written since.
                        let _ = lock(&store).and_then(|s| match s.thread(id)? {
                            Some(t) if t.status == "running" => s.set_thread_status(id, &was),
                            _ => Ok(()),
                        });
                        return Err(e);
                    }
                    // Behind any held replies, and held itself if the store refuses it.
                    Ok(unsaved.push_and_save(&store, id, Some(record), None).0)
                })
                .await;
            if let Some(reply) = on_sent {
                // Only a limit or an ended agent is the caller's business; store details stay in the tab.
                let _ = reply.send(sent.as_ref().map(|_| ()).map_err(|e| {
                    let why = format!("{e:#}");
                    if why.contains("running children") || why.contains("has ended") {
                        why
                    } else {
                        "the message couldn't be sent; the thread's tab says why".to_owned()
                    }
                }));
            }
            let _ = this.update(cx, |this, cx| match sent {
                Ok(left) => this.saved(left, cx),
                Err(e) => this.show_error(e, cx),
            });
        }));
    }
}
