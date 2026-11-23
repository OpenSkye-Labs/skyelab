//! Starting `claude` for a thread: a new thread, or resuming an ended session. Follow-ups are `thread_follow_up.rs`.

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

    /// Starts the account's agent (`claude` or `codex`) under `account`: a new thread row, or `resume`'s session.
    pub(in crate::workspace) fn launch(
        &mut self,
        text: String,
        attachments: Vec<skyelab_agent::Attachment>,
        account: Account,
        resume: Option<Thread>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.starting = true;
        self.stop_requested = false;
        self.launches += 1;
        self.prompt.update(cx, |p, cx| p.set_value("", window, cx));
        let record = Record::Prompt(super::composer::shown_prompt(&text, &attachments));
        self.push(&record, cx);
        let pick = self.composer.pick.clone();

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
        let base_branch = self.branch_picker.read(cx).base.clone();
        let parent = self.parent.as_ref().map(|p| p.0);
        let tools = crate::workspace::orchestrate::Tools::get(cx);
        let accounts_file = cx
            .try_global::<crate::workspace::AccountsFile>()
            .map(|f| f.0.clone());
        let is_new = resume.is_none();
        let mode = self.mode;
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
        // A checkpoint the last turn's end queued is taken before this turn's starting one.
        let prev = self.send_chain.take();
        self._task = Some(cx.spawn(async move |this, cx| {
            if let Some(prev) = prev {
                prev.await;
            }
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
                                    base_branch.as_deref(),
                                )
                            })
                            .transpose()
                            .context("creating the thread's worktree")?;
                        let project = cwd;
                        let cwd = worktree
                            .as_ref()
                            .map_or(project.clone(), |w| w.path.clone());
                        let mut thread = match resume {
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
                                    title: &title(if text.is_empty() {
                                        "Attachment"
                                    } else {
                                        &text
                                    }),
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
                        if is_new && mode != skyelab_agent::Mode::Default {
                            lock(&store)?.set_thread_permission_mode(thread.id, mode.as_str())?;
                            thread.permission_mode = mode.as_str().to_owned();
                        }
                        let _ = saved_tx.send(thread.clone());
                        // A resumed thread's held events go first, or its prompt would be stored ahead of them.
                        unsaved
                            .flush(&store, thread.id)
                            .context("earlier events of this thread aren't saved yet")?;
                        append(&mut *lock(&store)?, thread.id, &record)?;
                        if is_new {
                            lock(&store)?.set_thread_model(thread.id, &pick)?;
                        }
                        // Before the agent can touch anything: what "before this turn" rewinds to.
                        crate::workspace::thread_checkpoints::before_turn(
                            &cwd,
                            &crate::workspace::thread_checkpoints::key(&thread),
                        );
                        let grant = tools.and_then(|t| t.grant(thread.id, &account.id));
                        // skyelab-agent re-checks `account` against the folder at spawn and aborts if it moved.
                        let launch = Launch {
                            account: &account,
                            dir: &cwd,
                            resume: thread.agent_session_id.as_deref(), // a new row has none
                            prompt: &text,
                            mcp_config: grant.as_ref().map(|g| g.config_path()),
                            model: pick.model.as_deref(),
                            effort: pick.effort.as_deref(),
                            attachments: &attachments,
                            mode,
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
                                // Picked while it started: the agent (and a new row) began with the old mode.
                                if this.mode != mode {
                                    this.write_mode(cx);
                                }
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
        let moved = self.cwd != thread.cwd; // into its new worktree
        self.cwd = thread.cwd.clone();
        self.thread = Some(thread.clone());
        if moved {
            self.refresh_footer(cx);
        }
        cx.emit(ThreadViewEvent::Started(Box::new(thread.clone())));
        // Only for threads the user started: a setup script runs repo code in a terminal, which has the user's ssh
        // agent, so an agent's delegated thread must not trigger it.
        if let Some(cwd) = thread
            .worktree_path
            .clone()
            .filter(|_| thread.parent_thread_id.is_none())
        {
            let account_id = thread.account_id.clone();
            cx.emit(ThreadViewEvent::WorktreeCreated { cwd, account_id });
        }
        cx.notify();
    }
}
