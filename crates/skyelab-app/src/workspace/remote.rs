//! The Workspace side of sharing: the Remote tab, the status-bar item, and running remote agent calls through
//! the same `ThreadView` path a local prompt takes.

use super::Workspace;
pub(super) use super::remote_queue::StartQueue;
use super::remote_queue::{FIRST_LOAD_WAIT, MAX_WAITING_STARTS};
use super::thread::{SendPrompt, ThreadView};
use crate::remote::{
    AllowDevice, DenyDevice, DisconnectPeer, OpenRemote, Remote, RemoteCall, RemotePanel, Reply,
    RevokeDevice, ToggleSharing,
};
use gpui::{
    AppContext as _, Context, Entity, Focusable as _, IntoElement, Subscription, Task, WeakEntity,
    Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    dock::{DockPlacement, panel_handle},
    h_flex,
};
use skyelab_protocol::{ErrorCode, RpcError};
use std::time::Duration;

pub(super) struct RemoteHook {
    remote: Entity<Remote>,
    panel: Option<WeakEntity<RemotePanel>>,
    _calls: Task<()>,
    _observe: Subscription,
}

/// How long a remote follow-up or interrupt waits to reach the agent; under the daemon's own 30 s wait, so the
/// client hears this answer rather than a timeout.
const DELIVERY_WAIT: Duration = Duration::from_secs(20);

/// Sends `delivered`'s result to `reply`, or a timeout error once [`DELIVERY_WAIT`] passes.
fn answer_within(
    delivered: impl std::future::Future<Output = Result<(), RpcError>> + 'static,
    reply: Reply<()>,
    cx: &mut Context<Workspace>,
) {
    use futures::FutureExt as _;
    let timer = cx.background_executor().timer(DELIVERY_WAIT);
    cx.spawn(async move |_, _| {
        let answer = futures::select! {
            r = delivered.fuse() => r,
            () = timer.fuse() => Err(crate::remote::internal("the agent didn't take it in time")),
        };
        let _ = reply.send(answer);
    })
    .detach();
}

fn refused(message: impl Into<String>) -> RpcError {
    RpcError::new(ErrorCode::BadRequest, message.into())
}

pub(super) fn decision_of(a: skyelab_protocol::PermissionAnswer) -> skyelab_agent::Decision {
    use skyelab_protocol::PermissionAnswer as A;
    match a {
        A::AllowOnce => skyelab_agent::Decision::AllowOnce,
        A::AllowThread => skyelab_agent::Decision::AllowThread,
        A::Deny => skyelab_agent::Decision::Deny,
    }
}

fn not_open(thread_id: i64) -> RpcError {
    RpcError::new(
        ErrorCode::NotFound,
        format!("thread {thread_id} isn't running here (it ended, or its tab was closed)"),
    )
}

impl Workspace {
    /// Hooks up sharing: the status bar follows `remote`, and every call on `calls` runs on this window.
    pub fn set_remote(
        &mut self,
        remote: Entity<Remote>,
        calls: flume::Receiver<RemoteCall>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _calls = cx.spawn_in(window, async move |this, cx| {
            while let Ok(call) = calls.recv_async().await {
                if this
                    .update_in(cx, |this, window, cx| this.remote_call(call, window, cx))
                    .is_err()
                {
                    return; // window closed; dropping `call` tells the client the app is closing
                }
            }
        });
        self.remote = Some(RemoteHook {
            _observe: cx.observe(&remote, |_, _, cx| cx.notify()),
            remote,
            panel: None,
            _calls,
        });
        cx.notify();
    }

    pub(super) fn open_remote(
        &mut self,
        _: &OpenRemote,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(hook) = &mut self.remote else {
            return;
        };
        if let Some(panel) = hook.panel.as_ref().and_then(|p| p.upgrade()) {
            let id = panel.entity_id().into();
            self.dock
                .update(cx, |dock, cx| dock.select_panel(id, window, cx));
            return;
        }
        let remote = hook.remote.clone();
        let panel = cx.new(|cx| RemotePanel::new(remote, cx));
        hook.panel = Some(panel.downgrade());
        self.dock.update(cx, |dock, cx| {
            dock.add_panel_view(
                panel_handle(panel.clone()),
                DockPlacement::Center,
                None,
                window,
                cx,
            );
        });
        panel.read(cx).focus_handle(cx).focus(window, cx);
    }

    pub(super) fn toggle_sharing(
        &mut self,
        _: &ToggleSharing,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(hook) = &self.remote {
            hook.remote.update(cx, |r, cx| r.toggle(cx));
        }
    }

    pub(super) fn disconnect_peer(
        &mut self,
        a: &DisconnectPeer,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(hook) = &self.remote {
            hook.remote.update(cx, |r, cx| r.disconnect(a.id, cx));
        }
    }

    pub(super) fn allow_device(&mut self, a: &AllowDevice, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(hook) = &self.remote {
            hook.remote
                .update(cx, |r, cx| r.allow(&a.node_id, &a.key_hash, cx));
        }
    }

    pub(super) fn deny_device(&mut self, a: &DenyDevice, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(hook) = &self.remote {
            hook.remote
                .update(cx, |r, cx| r.deny(&a.node_id, &a.key_hash, cx));
        }
    }

    pub(super) fn revoke_device(
        &mut self,
        a: &RevokeDevice,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(hook) = &self.remote {
            hook.remote
                .update(cx, |r, cx| r.revoke(&a.node_id, &a.key_hash, cx));
        }
    }

    /// Whether sharing is switched on; `None` when this window has no sharing hook.
    pub(super) fn sharing_enabled(&self, cx: &gpui::App) -> Option<bool> {
        Some(self.remote.as_ref()?.remote.read(cx).enabled)
    }

    fn sharing_session(&self, cx: &Context<Self>) -> Option<u64> {
        self.remote.as_ref()?.remote.read(cx).sharing_session()
    }

    pub(super) fn open_view(&self, thread_id: i64, cx: &gpui::App) -> Option<Entity<ThreadView>> {
        self.open_threads
            .iter()
            .filter_map(|t| t.upgrade())
            .find(|t| t.read(cx).thread.as_ref().map(|t| t.id) == Some(thread_id))
    }

    fn remote_call(&mut self, call: RemoteCall, window: &mut Window, cx: &mut Context<Self>) {
        if call.abandoned() {
            return;
        }
        // Queued before the user turned sharing off: access was revoked, so it doesn't run.
        if self.sharing_session(cx).is_none() {
            return call.refuse(refused("sharing was turned off on this machine"));
        }
        match call {
            RemoteCall::Start {
                project_id,
                prompt,
                reply,
            } => self.remote_start(project_id, &prompt, reply, window, cx),
            RemoteCall::Send {
                thread_id,
                text,
                reply,
            } => self.remote_send(thread_id, &text, reply, window, cx),
            RemoteCall::Interrupt { thread_id, reply } => {
                self.remote_interrupt(thread_id, reply, cx)
            }
            RemoteCall::Answer {
                thread_id,
                request_id,
                decision,
                reply,
            } => {
                let Some(view) = self.open_view(thread_id, cx) else {
                    let _ = reply.send(Err(not_open(thread_id)));
                    return;
                };
                let late = reply.clone();
                // Replied once the agent took the answer, or refused it (already answered or withdrawn).
                let asked = view.update(cx, |v, cx| {
                    v.answer_then(
                        &request_id,
                        decision_of(decision),
                        move |r| _ = late.send(r.map_err(refused)),
                        cx,
                    )
                });
                if let Err(why) = asked {
                    let _ = reply.send(Err(refused(why)));
                }
            }
        }
    }

    /// Answers once the follow-up reached the agent (its store write and the session's input queue), or with why it
    /// didn't. A stopped thread resumes, and is answered once its agent runs.
    fn remote_send(
        &mut self,
        thread_id: i64,
        text: &str,
        reply: Reply<()>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.open_view(thread_id, cx) else {
            let _ = reply.send(Err(not_open(thread_id)));
            return;
        };
        let (sent, delivered) = flume::bounded(1);
        let (taken, unused) = view.update(cx, |v, cx| {
            v.on_sent = Some(sent);
            let taken = v.submit_remote(text, window, cx);
            (taken, v.on_sent.take())
        });
        match (taken, unused) {
            (Err(why), _) => _ = reply.send(Err(refused(why))),
            // No follow-up took it: the thread resumed, so the prompt goes in with the new agent.
            (Ok(()), Some(_)) => {
                let (started, resumed) = flume::bounded(1);
                self.reply_when_started(&view, move |r| _ = started.send(r), cx);
                let resumed = async move {
                    match resumed.recv_async().await {
                        Ok(r) => r.map(drop).map_err(refused),
                        Err(_) => Err(crate::remote::internal("the thread's tab closed")),
                    }
                };
                answer_within(resumed, reply, cx);
            }
            (Ok(()), None) => {
                let delivered = async move {
                    match delivered.recv_async().await {
                        Ok(r) => r.map_err(refused),
                        Err(_) => Err(crate::remote::internal("the message was dropped")),
                    }
                };
                answer_within(delivered, reply, cx);
            }
        }
    }

    /// Answers once the interrupt is in the agent's input, or with why it couldn't be.
    fn remote_interrupt(&mut self, thread_id: i64, reply: Reply<()>, cx: &mut Context<Self>) {
        let session = match self.open_view(thread_id, cx) {
            Some(view) => view.read(cx).session.clone(),
            None => {
                let _ = reply.send(Err(not_open(thread_id)));
                return;
            }
        };
        let (Some(session), Some(view)) = (session, self.open_view(thread_id, cx)) else {
            let _ = reply.send(Err(refused("the agent isn't running")));
            return;
        };
        let work = cx.background_spawn(async move { super::lock(&session)?.interrupt() });
        let view = view.downgrade();
        let mut async_cx = cx.to_async();
        let delivered = async move {
            let result = work.await;
            // The local tab shows a failure too, as a local interrupt does.
            if let Err(e) = &result {
                let shown = format!("{e:#}");
                let _ = view.update(&mut async_cx, |v, cx| {
                    v.error = Some(shown.into());
                    cx.notify();
                });
            }
            result.map_err(|e| refused(format!("{e:#}")))
        };
        answer_within(delivered, reply, cx);
    }

    /// Opens a new thread tab (without taking the user's focus) and sends `prompt` from it. Replies with the
    /// thread id once its row exists, or with why it was refused: an unbound folder never gets a row.
    pub(super) fn remote_start(
        &mut self,
        project_id: i64,
        prompt: &str,
        reply: crate::remote::Reply<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.starts.loading.is_some() {
            return self.queue_start(project_id, prompt, reply, window, cx);
        }
        let Some(project) = self.projects.iter().find(|p| p.id == project_id).cloned() else {
            let _ = reply.send(Err(RpcError::new(ErrorCode::NotFound, "no such project")));
            return;
        };
        let (config, backend) = (self.config.clone(), self.backend.clone());
        let view = cx.new(|cx| ThreadView::new(&project, config, backend, window, cx));
        if let Err(why) = view.update(cx, |v, cx| v.submit_remote(prompt, window, cx)) {
            let _ = reply.send(Err(refused(why)));
            return;
        }
        self.add_thread_tab(view.clone(), false, window, cx);
        self.reply_when_started(
            &view,
            move |r| _ = reply.send(r.map_err(crate::remote::internal)),
            cx,
        );
    }

    /// Calls `done` once `view`'s thread has a running agent (`Ok(id)`), or with why it didn't start.
    pub(super) fn reply_when_started(
        &mut self,
        view: &Entity<ThreadView>,
        done: impl FnOnce(Result<i64, &'static str>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let mut done = Some(done);
        // A launch already under way that ends with no agent failed, even on a thread that stays resumable.
        let began = view.read(cx).starting;
        self._thread_events
            .push(cx.observe(view, move |_, view, cx| {
                let view = view.read(cx);
                // The details (paths, store errors) stay in this machine's log and the thread's tab.
                let result = match (&view.thread, view.starting) {
                    (Some(t), _) if view.session.is_some() => Ok(t.id),
                    (Some(_), _) if view.read_only => Err("the agent didn't start"),
                    (Some(_), false) if began => Err("the agent didn't start"),
                    (None, false) => Err("the thread couldn't be created"),
                    _ => return,
                };
                if let Err(why) = result {
                    eprintln!("skyelab: start failed: {why}: {:?}", view.error);
                }
                if let Some(done) = done.take() {
                    done(result);
                }
            }));
    }

    /// Holds a start until the first project load ends, so its project can be found.
    fn queue_start(
        &mut self,
        project_id: i64,
        prompt: &str,
        reply: crate::remote::Reply<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.starts.calls.retain(|c| !c.2.is_disconnected());
        if self.starts.calls.len() >= MAX_WAITING_STARTS {
            let _ = reply.send(Err(refused("this machine is still starting; try again")));
            return;
        }
        let session = self.sharing_session(cx);
        let call = (project_id, prompt.to_owned(), reply, session);
        self.starts.calls.push(call);
        if self.starts._drain.is_some() {
            return;
        }
        let timer = cx.background_executor().timer(FIRST_LOAD_WAIT);
        self.starts._timeout = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, _| this.starts.loaded());
        }));
        let wait = self.starts.wait.clone();
        self.starts._drain = Some(cx.spawn_in(window, async move |this, cx| {
            let _ = wait.recv_async().await; // Err once the load ended: the sender is gone
            let _ = this.update_in(cx, |this, window, cx| {
                for (id, prompt, reply, session) in std::mem::take(&mut this.starts.calls) {
                    if reply.is_disconnected() {
                        continue;
                    }
                    // Sharing turned off (or off and on again) while it waited: that access was revoked.
                    if this.sharing_session(cx) != session {
                        let _ = reply.send(Err(refused("sharing was turned off on this machine")));
                        continue;
                    }
                    this.remote_start(id, &prompt, reply, window, cx);
                }
            });
        }));
    }

    pub(super) fn render_remote_status(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        let remote = self.remote.as_ref()?.remote.read(cx);
        // A device waiting for approval matters more than the sharing details.
        let (label, dot) = match remote.requests.first() {
            Some(r) => (Remote::request_label(r).into(), cx.theme().warning),
            None => (remote.status.label(), remote.status.color(cx.theme())),
        };
        Some(
            Button::new("status-remote")
                .ghost()
                .xsmall()
                .tooltip("Remote: sharing this machine on your tailnet")
                .child(
                    h_flex()
                        .gap_1p5()
                        .child(div().size(px(6.)).rounded_full().bg(dot))
                        .child(label),
                )
                .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenRemote), cx)),
        )
    }
}

impl ThreadView {
    /// A remote client's prompt or follow-up, sent exactly as cmd-enter sends it. The user's draft is kept.
    /// `Err` says why it was refused (unbound folder, ended thread, rebound folder).
    pub(super) fn submit_remote(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if text.trim().is_empty() {
            return Err("the prompt is empty".into());
        }
        let draft = self.prompt.read(cx).value().to_string();
        // Cleared so a refusal that sets the same message again (a rebound folder) still shows up as new.
        let error = self.error.take();
        let refused = self.resume_refused.take();
        self.prompt
            .update(cx, |p, cx| p.set_value(text, window, cx));
        self.send_prompt(&SendPrompt, window, cx);
        // send_prompt clears the prompt exactly when it took the text.
        let taken = self.prompt.read(cx).value().is_empty();
        self.prompt
            .update(cx, |p, cx| p.set_value(draft, window, cx));
        if taken {
            return Ok(());
        }
        // A stopped thread's resume was refused: its folder no longer resolves to the thread's account.
        if let Some(why) = &self.resume_refused {
            self.error = error;
            return Err(why.to_string());
        }
        self.resume_refused = refused;
        let rebound = self.error.is_some(); // the only refusal send_prompt explains in `error`
        if !rebound {
            self.error = error;
        }
        Err(if self.needs_binding {
            "this folder isn't bound to an account on this machine; bind it there first"
        } else if rebound {
            "this folder is now bound to another account; start a new thread"
        } else if self.read_only {
            "this thread has ended"
        } else {
            "this thread is still starting; try again"
        }
        .into())
    }
}

#[cfg(test)]
impl Workspace {
    pub(crate) fn project_ids(&self) -> Vec<i64> {
        self.projects.iter().map(|p| p.id).collect()
    }

    pub(crate) fn open_thread_ids(&self, cx: &gpui::App) -> Vec<i64> {
        self.open_threads
            .iter()
            .filter_map(|t| t.upgrade())
            .filter_map(|t| t.read(cx).thread.as_ref().map(|t| t.id))
            .collect()
    }
}
