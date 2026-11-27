//! Starting `claude` for a thread: a new thread, or resuming an ended session. Follow-ups are `thread_follow_up.rs`.

use super::saves::append;
use super::{Record, ThreadView, ThreadViewEvent};
use crate::workspace::lock;
use anyhow::Context as _;
use gpui::{AppContext as _, Context, Window};
use skyelab_agent::Launch;
use skyelab_core::{Account, Provider};
use skyelab_store::{NewThread, Thread};

/// Why a resume is refused while another thread runs the same agent session (imported twice, or forked rows).
pub(super) const BUSY_ELSEWHERE: &str = "Another thread is running this agent session right now. Stop it there first, then send this again.";

impl ThreadView {
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
        self.prompt.update(cx, |p, cx| p.set_value("", window, cx));
        self.restore_draft(window, cx); // set_value emits no Change
        let shown = super::composer::shown_prompt(&text, &attachments);
        self.keep_images(&shown, &attachments);
        self.push(&Record::Prompt(shown), cx);
        // The prompt just cancelled any resume scheduled for a usage limit's reset (`push`).
        let resume = resume.map(|t| Thread {
            limit_resume_at: None,
            ..t
        });
        self.start_agent(text, attachments, account, resume, false, cx);
    }

    /// The resumed session was gone: start `text` again in a new session. Its prompt is already shown and stored.
    /// The row keeps the dead session id until the new session reports its own, so a failed start leaves the thread
    /// able to send again. `Err` says why nothing started.
    pub(super) fn start_fresh(
        &mut self,
        text: String,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let mut thread = self.thread.clone().ok_or("the thread has no row")?;
        match self.config.resolve(&self.cwd).cloned() {
            Some(account) if account.id == thread.account_id => {
                thread.agent_session_id = None;
                // shortcut: the rerun sends the prompt's text only, not its attachments; keep them if that matters.
                self.start_agent(text, Vec::new(), account, Some(thread), true, cx);
                Ok(())
            }
            _ => Err(format!(
                "The agent's earlier session couldn't be found, and {} isn't bound to {} any more, so it can't \
                 start a new one. Bind the folder again, then send a message.",
                self.cwd.display(),
                thread.account_id
            )),
        }
    }

    /// Starts the agent; `again` reruns a prompt that is already shown and stored (no second prompt row).
    pub(super) fn start_agent(
        &mut self,
        text: String,
        attachments: Vec<skyelab_agent::Attachment>,
        account: Account,
        resume: Option<Thread>,
        again: bool,
        cx: &mut Context<Self>,
    ) {
        self.rerun = None;
        self.resumed_prompt = resume
            .as_ref()
            .filter(|t| t.agent_session_id.is_some())
            .map(|_| text.clone());
        self.starting = true;
        self.startup = Some(crate::workspace::thread_hooks::Startup {
            provider: account.provider,
            resuming: self.resumed_prompt.is_some(),
        });
        // The last process's hooks and MCP servers aren't this one's.
        self.hooks.clear();
        self.details.mcp.clear();
        self.stop_requested = false;
        self.launches += 1;
        self.tick_while_working(cx); // a rerun pushes no prompt, so nothing else restarts it
        let record = Record::Prompt(super::composer::shown_prompt(&text, &attachments));
        let pick = self.composer.pick.clone();

        let bin = match account.provider {
            Provider::Claude => self.backend.claude_bin.clone(),
            Provider::Codex => crate::workspace::github::codex_bin(cx),
            Provider::Cursor => crate::workspace::github::cursor_bin(cx),
            Provider::Antigravity => crate::workspace::github::antigravity_bin(cx),
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
        let pin = self.wt.pin.clone();
        let parent = self.parent.as_ref().map(|p| p.0);
        let remote_parent = self.remote_parent.clone().filter(|_| resume.is_none());
        let compare_group = self.compare_group.filter(|_| resume.is_none());
        // A side chat is a read-only helper: Plan mode and none of SkyeLab's tools, whatever its row or view says.
        let side = self.is_side_chat() || resume.as_ref().is_some_and(|t| t.side_of.is_some());
        let tools = crate::workspace::orchestrate::Tools::get(cx).filter(|_| !side);
        let accounts_file = cx
            .try_global::<crate::workspace::AccountsFile>()
            .map(|f| f.0.clone());
        let is_new = resume.is_none();
        let scheduled_task = self.scheduled_task.filter(|_| is_new);
        // Cleared once the launch succeeds, so a failed first send keeps it for the next.
        let link_pr = if is_new {
            self.composer.extras.pending_pr.clone()
        } else {
            None
        };
        let mode = if side {
            skyelab_agent::Mode::Plan
        } else {
            self.mode
        };
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
                                    pin.as_ref(),
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
                                // Checked and marked under one lock, so two resumes of one session can't both pass.
                                let store = lock(&store)?;
                                if let Some(session) = thread.agent_session_id.as_deref()
                                    && store.session_busy_elsewhere(thread.id, session)?
                                {
                                    anyhow::bail!(BUSY_ELSEWHERE);
                                }
                                // A resumed child takes a slot again, under the same lock as its status.
                                use crate::workspace::child_slots::{claim_for, delegated_parent};
                                if let Some(key) =
                                    delegated_parent(&store, thread.id, thread.parent_thread_id)?
                                {
                                    claim_for(&store, &key, Some(thread.id))?;
                                }
                                store.set_thread_status(thread.id, "starting")?;
                                thread
                            }
                            None => crate::workspace::worktree::save(
                                &store,
                                &project,
                                &NewThread {
                                    project_id,
                                    title: &crate::workspace::thread_extras::title::from_prompt(
                                        if text.is_empty() { "Attachment" } else { &text },
                                    ),
                                    cwd: &cwd,
                                    account_id: &account.id,
                                    status: "starting",
                                },
                                worktree,
                                parent,
                                remote_parent.as_ref(),
                            )?,
                        };
                        // A new row, or a resumed thread's slot: neither may stay busy if this launch fails.
                        claimed = Some(thread.id);
                        // Grouped before the row is announced, so the sidebar folds it in at once.
                        if let Some(group) = compare_group {
                            lock(&store)?.set_thread_compare_group(thread.id, group)?;
                            thread.compare_group = Some(group);
                        }
                        // Best effort: a link that can't be saved doesn't stop the review.
                        if let Some(url) = &link_pr {
                            match lock(&store)
                                .and_then(|s| s.set_thread_pr_url(thread.id, Some(url)))
                            {
                                Ok(()) => thread.pr_url = Some(url.clone()),
                                Err(e) => eprintln!("skyelab: linking the PR failed: {e:#}"),
                            }
                        }
                        if is_new && mode != skyelab_agent::Mode::Default {
                            lock(&store)?.set_thread_permission_mode(thread.id, mode.as_str())?;
                            thread.permission_mode = mode.as_str().to_owned();
                        }
                        if let Some(task) = scheduled_task {
                            lock(&store)?.set_thread_scheduled_task(thread.id, task)?;
                            thread.scheduled_task_id = Some(task);
                        }
                        let _ = saved_tx.send(thread.clone());
                        // A resumed thread's held events go first, or its prompt would be stored ahead of them.
                        unsaved
                            .flush(&store, thread.id)
                            .context("earlier events of this thread aren't saved yet")?;
                        if !again {
                            append(&mut *lock(&store)?, thread.id, &record)?;
                        }
                        if is_new {
                            lock(&store)?.set_thread_model(thread.id, &pick)?;
                        }
                        // Before the agent can touch anything: what "before this turn" rewinds to.
                        crate::workspace::thread_checkpoints::before_turn(
                            &cwd,
                            &crate::workspace::thread_checkpoints::key(&thread),
                        );
                        let grant = tools.and_then(|t| t.grant(thread.id, &account.id));
                        // A fork's (or side chat's) first session gets its context ahead of the message (stored as typed).
                        let context = match (
                            thread.forked_from.or(thread.side_of),
                            &thread.agent_session_id,
                        ) {
                            (Some(_), None) => lock(&store)?.fork_context(thread.id)?,
                            _ => None,
                        };
                        let prompt = context.map_or_else(|| text.clone(), |c| c + &text);
                        // skyelab-agent re-checks `account` against the folder at spawn and aborts if it moved.
                        let launch = Launch {
                            account: &account,
                            dir: &cwd,
                            resume: thread.agent_session_id.as_deref(), // a new row has none
                            prompt: &prompt,
                            mcp_config: grant.as_ref().map(|g| g.config_path()),
                            model: pick.model.as_deref(),
                            effort: pick.effort.as_deref(),
                            attachments: &attachments,
                            mode,
                        };
                        // The binding as saved right now: a rebind may have landed while this launch ran. An
                        // unreadable file binds nothing, so the check below refuses.
                        let current =
                            crate::workspace::accounts_now(accounts_file.as_deref(), &config);
                        let session = skyelab_agent::start_with_bin(&bin, &current, &launch);
                        let status = if session.is_ok() { "running" } else { "failed" };
                        lock(&store)?.set_thread_status(thread.id, status)?;
                        // From here on, a closed tab can't leave the row running with nothing behind it.
                        let session = session
                            .map(|s| super::teardown::NewSession::new(s, store.clone(), thread.id));
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
                        this.composer.extras.pending_pr = None;
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
            self._watch =
                crate::workspace::folder_watch::watch(&self.cwd, cx, Self::folder_changed);
            self.refresh_footer(cx);
        }
        cx.emit(ThreadViewEvent::Started(Box::new(thread.clone())));
        // Only for threads the user started: a setup script runs repo code in a terminal, which has the user's ssh
        // agent, so an agent's delegated thread must not trigger it.
        if let Some(cwd) = thread
            .worktree_path
            .clone()
            .filter(|_| thread.parent_thread_id.is_none() && self.remote_parent.is_none())
        {
            let account_id = thread.account_id.clone();
            cx.emit(ThreadViewEvent::WorktreeCreated { cwd, account_id });
        }
        cx.notify();
    }
}
