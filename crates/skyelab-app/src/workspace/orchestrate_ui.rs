//! The Workspace answers delegated starts and sends on the UI thread, through the same `ThreadView` path a
//! local prompt takes. The account rule is checked again here, against the config the launch will use.

use super::orchestrate::{OrchCall, Reply, StartRequest};
use super::thread::{ChildLink, ThreadView};
use super::{Workspace, lock};
use gpui::{
    App, AppContext as _, Context, Entity, EntityId, Global, SharedString, Subscription,
    WeakEntity, Window,
};
use serde_json::json;
use skyelab_agent::mcp::{Caller, HTML_RENDERED, HtmlTool};
use skyelab_core::delegation::check_limits;
use std::collections::HashMap;

/// A delegated start, by (caller thread, its request id).
type StartKey = (i64, String);

/// What a retry of a start gets: the thread's id once known, else its tab while it starts.
struct DelegatedStart {
    view: WeakEntity<ThreadView>,
    id: Option<i64>,
}

/// Delegated starts by request id, and the waits that answer them. App-wide, not per window: a start outlives the
/// window that took it, and a retry served by another window still gets the thread the first call started.
#[derive(Default)]
pub(super) struct DelegatedStarts {
    starts: HashMap<StartKey, DelegatedStart>,
    waits: HashMap<EntityId, Vec<Subscription>>,
}

impl Global for DelegatedStarts {}

impl DelegatedStarts {
    /// A thread closed: its pending answers go (the caller hears it ended), like any wait on a closed tab.
    pub(super) fn forget(view: EntityId, cx: &mut App) {
        if cx.has_global::<Self>() {
            cx.global_mut::<Self>().waits.remove(&view);
        }
    }
}

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

    pub(super) fn orchestrate(
        &mut self,
        call: OrchCall,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
                let sent = self.caller_live(&caller, cx).and_then(|()| {
                    self.delegated_send(&caller, thread_id, &text, reply.clone(), window, cx)
                });
                if let Err(why) = sent {
                    let _ = reply.send(Err(why));
                }
            }
            OrchCall::Browser {
                caller,
                tool,
                reply,
            } => {
                if reply.is_disconnected() {
                    return;
                }
                match self.caller_live(&caller, cx) {
                    Ok(()) => self.browser_call(caller.thread_id, tool, reply, window, cx),
                    Err(why) => {
                        let _ = reply.send(Err(why));
                    }
                }
            }
            OrchCall::Device {
                caller,
                tool,
                reply,
            } => {
                if reply.is_disconnected() {
                    return;
                }
                match self.caller_live(&caller, cx) {
                    Ok(()) => self.device_call(caller.thread_id, tool, reply, window, cx),
                    Err(why) => {
                        let _ = reply.send(Err(why));
                    }
                }
            }
            OrchCall::Html {
                caller,
                tool,
                reply,
            } => {
                if reply.is_disconnected() {
                    return;
                }
                if let Err(why) = self.caller_live(&caller, cx) {
                    let _ = reply.send(Err(why));
                    return;
                }
                match tool {
                    // The call itself, stored with the thread's events, is what the timeline draws.
                    HtmlTool::Render { .. } => {
                        let _ = reply.send(Ok(json!({"message": HTML_RENDERED})));
                    }
                    HtmlTool::Preview { html, width, dark } => {
                        super::html_card::preview(&html, width, dark, reply, window, cx)
                    }
                }
            }
        }
    }

    fn caller_live(&self, caller: &Caller, cx: &App) -> Result<(), String> {
        match self.open_view(caller.thread_id, cx) {
            Some(view) if view.read(cx).is_live() => Ok(()),
            _ => Err(super::orchestrate::CALLER_GONE.into()),
        }
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
        let known = key.as_ref().and_then(|k| {
            let start = cx.try_global::<DelegatedStarts>()?.starts.get(k)?;
            Some((start.id, start.view.upgrade()))
        });
        if let Some((id, view)) = known {
            if let Some(id) = id {
                let _ = reply.send(Ok(id));
                return Ok(());
            }
            // Still on its way: the same thread once it has its row. One that failed before its row starts afresh.
            if let Some(view) = view.filter(|v| row_result(v.read(cx)).is_none_or(|r| r.is_ok())) {
                reply_when_row(&view, key, reply, cx);
                return Ok(());
            }
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
        // Refused here so a full parent gets no tab; the claim inside the launch, under the store lock, decides.
        let busy = lock(&self.backend.store)
            .and_then(|s| super::child_slots::busy(&s, caller.thread_id, None))
            .map_err(|e| format!("{e:#}"))?;
        check_limits(*depth, busy)?;
        let parent_title: SharedString = self.open_view(caller.thread_id, cx).map_or_else(
            || format!("thread {}", caller.thread_id).into(),
            |v| v.read(cx).title(),
        );
        let (config, backend) = (self.config.clone(), self.backend.clone());
        let view = cx.new(|cx| {
            let mut v = ThreadView::new(&project, config, backend, window, cx);
            v.parent = Some((caller.thread_id, parent_title));
            v.wt.use_worktree = *worktree;
            // An agent-started child never gets Full access: Ask, or Plan when that is the project's default.
            if v.mode != skyelab_agent::Mode::Plan {
                v.mode = skyelab_agent::Mode::Default;
            }
            v.required_account = Some(account);
            v
        });
        view.update(cx, |v, cx| v.submit_remote(prompt, window, cx))?;
        self.add_thread_tab(view.clone(), false, window, cx);
        if let Some(key) = &key {
            // Only a live caller can retry, so its entries are the only ones worth keeping.
            let callers: Vec<i64> = cx
                .try_global::<DelegatedStarts>()
                .map(|d| d.starts.keys().map(|k| k.0).collect())
                .unwrap_or_default();
            let live: std::collections::HashSet<i64> = callers
                .into_iter()
                .filter(|c| self.open_view(*c, cx).is_some())
                .collect();
            let starts = &mut cx.default_global::<DelegatedStarts>().starts;
            starts.retain(|k, _| live.contains(&k.0));
            let start = DelegatedStart {
                view: view.downgrade(),
                id: None,
            };
            starts.insert(key.clone(), start);
        }
        reply_when_row(&view, key, reply, cx);
        Ok(())
    }

    /// Watches `view` until `done` returns true, then drops the watch.
    pub(super) fn wait_on(
        &mut self,
        view: &Entity<ThreadView>,
        cx: &mut Context<Self>,
        mut done: impl FnMut(&mut Self, Entity<ThreadView>, &mut Context<Self>) -> bool + 'static,
    ) {
        let key = self.next_waiter;
        self.next_waiter += 1;
        let watch = cx.observe(view, move |this, view, cx| {
            if done(this, view, cx) {
                this.waiters.remove(&key);
            }
        });
        self.waiters.insert(key, (view.entity_id(), watch));
    }

    fn delegated_send(
        &mut self,
        caller: &Caller,
        thread_id: i64,
        text: &str,
        reply: Reply<()>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let view = self.open_view(thread_id, cx).ok_or_else(|| {
            format!("thread {thread_id} isn't running (it ended, or its tab was closed)")
        })?;
        let (thread, starting, running) = {
            let v = view.read(cx);
            (v.thread.clone(), v.starting, v.session.is_some())
        };
        let thread = thread.filter(|_| !starting).ok_or_else(|| {
            format!(
                "thread {thread_id} is still starting; wait for it (thread_status), then send again"
            )
        })?;
        // A message to an ended thread would resume it: past the children limit, and past the user's Stop.
        if !running {
            return Err(format!(
                "thread {thread_id} isn't running (it ended or was stopped); start a new thread instead"
            ));
        }
        let project = self
            .projects
            .iter()
            .find(|p| p.id == thread.project_id)
            .ok_or("that thread's project is gone")?;
        self.config
            .check_drive(&caller.account_id, &thread.account_id, &project.path)?;
        // The answer waits for the follow-up's store write, where an idle child's new turn claims its slot, so a
        // send past the children limit is refused to the caller, not just in the child's tab.
        view.update(cx, |v, cx| {
            v.on_sent = Some(reply);
            let taken = v.submit_remote(text, window, cx);
            if let Some(reply) = v.on_sent.take() {
                let _ = reply.send(taken); // no follow-up took it
            }
        });
        Ok(())
    }

    /// A delegated thread got its row: the parent's tab lists it.
    pub(super) fn child_started(&mut self, child: &skyelab_store::Thread, cx: &mut Context<Self>) {
        let Some(parent) = child.parent_thread_id.and_then(|p| self.open_view(p, cx)) else {
            return;
        };
        let link = ChildLink {
            id: child.id,
            title: child.title.clone().into(),
            status: crate::ui::Status::parse(&child.status),
            started_ms: child.created_at,
        };
        parent.update(cx, |p, cx| {
            if !p.children.iter().any(|c| c.id == link.id) {
                p.children.push(link);
                cx.notify();
            }
        });
    }
}

/// `Some` once `view`'s start has an answer: its row's id, or why it got none. Only a limit's reason goes to the
/// agent; other details (paths, store errors) stay in the tab.
fn row_result(view: &ThreadView) -> Option<Result<i64, String>> {
    match (&view.thread, view.starting) {
        (Some(t), _) => Some(Ok(t.id)),
        (None, false) => Some(Err(match &view.error {
            Some(e) if e.contains("running children") => e.to_string(),
            _ => "the thread couldn't be created; its tab says why".to_owned(),
        })),
        (None, true) => None,
    }
}

/// Answers with the thread's id once its row exists, before its agent is up, so a slow start can't outlast the
/// agent's wait and leave a child it never heard of. Or with why the row was never made. The wait is app-wide, so the
/// window that took the call may close meanwhile.
fn reply_when_row(
    view: &Entity<ThreadView>,
    key: Option<StartKey>,
    reply: Reply<i64>,
    cx: &mut App,
) {
    let answer = move |cx: &mut App, result: Result<i64, String>| {
        if let (Ok(id), Some(k)) = (&result, &key)
            && let Some(start) = cx.default_global::<DelegatedStarts>().starts.get_mut(k)
        {
            start.id = Some(*id); // a retry after the tab closed still names this thread
        }
        let _ = reply.send(result);
    };
    if let Some(result) = row_result(view.read(cx)) {
        return answer(cx, result);
    }
    let mut answer = Some(answer);
    let id = view.entity_id();
    let watch = cx.observe(view, move |view, cx| {
        let Some(result) = row_result(view.read(cx)) else {
            return;
        };
        if let Some(answer) = answer.take() {
            answer(cx, result);
        }
        // Every wait on this view has its answer now; drop them once this callback is done.
        cx.defer(move |cx| DelegatedStarts::forget(id, cx));
    });
    let waits = &mut cx.default_global::<DelegatedStarts>().waits;
    waits.entry(id).or_default().push(watch);
}
