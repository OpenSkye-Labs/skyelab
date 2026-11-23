//! The Workspace answers delegated starts and sends on the UI thread, through the same `ThreadView` path a
//! local prompt takes. The account rule is checked again here, against the config the launch will use.

use super::orchestrate::{OrchCall, Reply, StartRequest};
use super::thread::{ChildLink, ThreadView};
use super::{Workspace, lock};
use gpui::{App, AppContext as _, Context, Entity, SharedString, Window};
use skyelab_agent::mcp::Caller;
use skyelab_core::delegation::{check_children, check_limits, is_busy};

impl Workspace {
    /// Every delegated start and send on `calls` runs on this window.
    pub fn set_orchestrator(
        &mut self,
        calls: flume::Receiver<OrchCall>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._orchestrate = Some(cx.spawn_in(window, async move |this, cx| {
            while let Ok(call) = calls.recv_async().await {
                if this
                    .update_in(cx, |this, window, cx| this.orchestrate(call, window, cx))
                    .is_err()
                {
                    return; // window closed; the agent's call times out
                }
            }
        }));
    }

    fn orchestrate(&mut self, call: OrchCall, window: &mut Window, cx: &mut Context<Self>) {
        match call {
            OrchCall::Start { request, reply } => {
                if reply.is_disconnected() {
                    return; // the agent stopped waiting
                }
                let started = self
                    .caller_live(&request.caller, cx)
                    .and_then(|()| self.delegate(&request, reply.clone(), window, cx));
                if let Err(why) = started {
                    let _ = reply.send(Err(why));
                }
            }
            OrchCall::Send {
                caller,
                thread_id,
                text,
                reply,
            } => {
                if reply.is_disconnected() {
                    return;
                }
                let sent = self
                    .caller_live(&caller, cx)
                    .and_then(|()| self.delegated_send(&caller, thread_id, &text, window, cx));
                let _ = reply.send(sent);
            }
            OrchCall::Browser {
                caller,
                tool,
                reply,
            } => {
                if !reply.is_disconnected() {
                    self.browser_call(caller.thread_id, tool, reply, window, cx);
                }
            }
        }
    }

    /// A call queued before its thread was stopped or closed still arrives: it must not start or drive anything.
    fn caller_live(&self, caller: &Caller, cx: &App) -> Result<(), String> {
        match self.open_view(caller.thread_id, cx) {
            Some(view) if view.read(cx).is_live() => Ok(()),
            _ => Err(
                "this thread isn't running here any more (it was stopped, or its tab closed); nothing was done"
                    .into(),
            ),
        }
    }

    /// Busy children of `parent` other than `except`, from the store.
    fn busy_children(&self, parent: i64, except: Option<i64>) -> Result<usize, String> {
        let store = lock(&self.backend.store).map_err(|e| e.to_string())?;
        let kids = store.child_threads(parent).map_err(|e| format!("{e:#}"))?;
        Ok(kids
            .iter()
            .filter(|t| Some(t.id) != except && is_busy(&t.status))
            .count())
    }

    fn delegate(
        &mut self,
        request: &StartRequest,
        reply: Reply<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let StartRequest {
            caller,
            depth,
            project_id,
            prompt,
            worktree,
            request_id,
        } = request;
        let key = request_id.clone().map(|r| (caller.thread_id, r));
        // A retry of a start that is still on its way, or that got its row: the same thread, not a second one.
        if let Some(view) = key
            .as_ref()
            .and_then(|k| self.delegated.get(k))
            .and_then(|v| v.upgrade())
            .filter(|v| row_result(v.read(cx)).is_none_or(|r| r.is_ok()))
        {
            self.reply_when_row(&view, reply, cx);
            return Ok(());
        }
        let project = self
            .projects
            .iter()
            .find(|p| p.id == *project_id)
            .cloned()
            .ok_or_else(|| format!("no project {project_id}; call list_projects"))?;
        let account = self
            .config
            .delegation_account(&caller.account_id, &project.path)?
            .id
            .clone();
        // Counted now, not when the call was queued. The claim inside the launch, under the store lock, decides.
        check_limits(*depth, self.busy_children(caller.thread_id, None)?)?;
        let parent_title: SharedString = self.open_view(caller.thread_id, cx).map_or_else(
            || format!("thread {}", caller.thread_id).into(),
            |v| v.read(cx).title(),
        );
        let (config, backend) = (self.config.clone(), self.backend.clone());
        let view = cx.new(|cx| {
            let mut v = ThreadView::new(&project, config, backend, window, cx);
            v.parent = Some((caller.thread_id, parent_title));
            v.wt.use_worktree = *worktree;
            v.required_account = Some(account);
            v
        });
        view.update(cx, |v, cx| v.submit_remote(prompt, window, cx))?;
        self.add_thread_tab(view.clone(), false, window, cx);
        if let Some(key) = key {
            self.delegated.retain(|_, v| v.upgrade().is_some());
            self.delegated.insert(key, view.downgrade());
        }
        self.reply_when_row(&view, reply, cx);
        Ok(())
    }

    /// Answers with the thread's id once its row exists, before its agent is up, so a slow start can't outlast
    /// the agent's wait and leave a child it never heard of. Or with why the row was never made.
    fn reply_when_row(
        &mut self,
        view: &Entity<ThreadView>,
        reply: Reply<i64>,
        cx: &mut Context<Self>,
    ) {
        if let Some(result) = row_result(view.read(cx)) {
            let _ = reply.send(result);
            return;
        }
        let mut reply = Some(reply);
        self._thread_events
            .push(cx.observe(view, move |_, view, cx| {
                if let Some(result) = row_result(view.read(cx))
                    && let Some(reply) = reply.take()
                {
                    let _ = reply.send(result);
                }
            }));
    }

    fn delegated_send(
        &mut self,
        caller: &Caller,
        thread_id: i64,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let view = self.open_view(thread_id, cx).ok_or_else(|| {
            format!("thread {thread_id} isn't running (it ended, or its tab was closed)")
        })?;
        let (thread, running) = {
            let v = view.read(cx);
            (v.thread.clone(), v.session.is_some() && !v.starting)
        };
        let thread = thread.ok_or("that thread is still starting")?;
        // A message to an ended thread would resume it: past the children limit, and past the user's Stop.
        if !running {
            return Err(format!(
                "thread {thread_id} isn't running (it ended or was stopped); start a new thread instead"
            ));
        }
        // A new turn for an idle child takes a slot. The launch claims it; this tells the caller when it can't.
        if let Some(parent) = thread.parent_thread_id {
            let status = lock(&self.backend.store)
                .and_then(|s| s.thread(thread_id))
                .map_err(|e| format!("{e:#}"))?
                .map(|t| t.status)
                .unwrap_or_default();
            if !is_busy(&status) {
                check_children(self.busy_children(parent, Some(thread_id))?)
                    .map_err(|why| format!("thread {thread_id}'s parent: {why}"))?;
            }
        }
        let project = self
            .projects
            .iter()
            .find(|p| p.id == thread.project_id)
            .ok_or("that thread's project is gone")?;
        self.config
            .check_drive(&caller.account_id, &thread.account_id, &project.path)?;
        view.update(cx, |v, cx| v.submit_remote(text, window, cx))
    }

    /// A delegated thread got its row: the parent's tab lists it.
    pub(super) fn child_started(&mut self, child: &skyelab_store::Thread, cx: &mut Context<Self>) {
        let Some(parent) = child.parent_thread_id.and_then(|p| self.open_view(p, cx)) else {
            return;
        };
        let link = ChildLink {
            id: child.id,
            title: child.title.clone().into(),
        };
        parent.update(cx, |p, cx| {
            if !p.children.iter().any(|c| c.id == link.id) {
                p.children.push(link);
                cx.notify();
            }
        });
    }
}

/// `Some` once `view`'s start has an answer: its row's id, or why it got none.
fn row_result(view: &ThreadView) -> Option<Result<i64, String>> {
    match (&view.thread, view.starting) {
        (Some(t), _) => Some(Ok(t.id)),
        (None, false) => Some(Err(view.error.as_ref().map_or_else(
            || "the thread couldn't be created".to_owned(),
            |e| e.to_string(),
        ))),
        (None, true) => None,
    }
}
