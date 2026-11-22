//! The Workspace side of sharing: the Remote tab, the status-bar item, and running remote agent calls through
//! the same `ThreadView` path a local prompt takes.

use super::Workspace;
use super::thread::{Interrupt, SendPrompt, ThreadView};
use crate::remote::{
    DisconnectPeer, OpenRemote, Remote, RemoteCall, RemotePanel, Status, ToggleSharing,
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

pub(super) struct RemoteHook {
    remote: Entity<Remote>,
    panel: Option<WeakEntity<RemotePanel>>,
    _calls: Task<()>,
    _observe: Subscription,
}

fn refused(message: impl Into<String>) -> RpcError {
    RpcError::new(ErrorCode::BadRequest, message.into())
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

    fn open_view(&self, thread_id: i64, cx: &Context<Self>) -> Option<Entity<ThreadView>> {
        self.open_threads
            .iter()
            .filter_map(|t| t.upgrade())
            .find(|t| t.read(cx).thread.as_ref().map(|t| t.id) == Some(thread_id))
    }

    fn remote_call(&mut self, call: RemoteCall, window: &mut Window, cx: &mut Context<Self>) {
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
            } => {
                let sent = match self.open_view(thread_id, cx) {
                    Some(view) => view
                        .update(cx, |v, cx| v.submit_remote(&text, window, cx))
                        .map_err(refused),
                    None => Err(not_open(thread_id)),
                };
                let _ = reply.send(sent);
            }
            RemoteCall::Interrupt { thread_id, reply } => {
                let done = match self.open_view(thread_id, cx) {
                    Some(view) => view.update(cx, |v, cx| {
                        if v.session.is_none() {
                            return Err(refused("the agent isn't running"));
                        }
                        v.interrupt(&Interrupt, window, cx);
                        Ok(())
                    }),
                    None => Err(not_open(thread_id)),
                };
                let _ = reply.send(done);
            }
        }
    }

    /// Opens a new thread tab (without taking the user's focus) and sends `prompt` from it. Replies with the
    /// thread id once its row exists, or with why it was refused: an unbound folder never gets a row.
    fn remote_start(
        &mut self,
        project_id: i64,
        prompt: &str,
        reply: crate::remote::Reply<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        let mut reply = Some(reply);
        let mut answer = move |view: &ThreadView| {
            let result = match (&view.thread, view.starting) {
                (Some(t), _) => Ok(t.id),
                (None, true) => return,
                (None, false) => Err(crate::remote::internal(
                    view.error.as_deref().unwrap_or("the thread didn't start"),
                )),
            };
            if let Some(reply) = reply.take() {
                let _ = reply.send(result);
            }
        };
        self._thread_events
            .push(cx.observe(&view, move |_, view, cx| answer(view.read(cx))));
    }

    pub(super) fn render_remote_status(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        let remote = self.remote.as_ref()?.remote.read(cx);
        let label = remote.status.label()?;
        let theme = cx.theme();
        let dot = match remote.status {
            Status::Sharing { .. } => theme.success,
            Status::Failed(_) => theme.danger,
            _ => theme.muted_foreground,
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
        let error = self.error.clone();
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
        Err(if self.needs_binding {
            "this folder isn't bound to an account on this machine; bind it there first".into()
        } else if self.error != error {
            self.error.as_deref().unwrap_or_default().to_owned()
        } else if self.read_only {
            "this thread has ended".into()
        } else {
            "this thread is still starting; try again".into()
        })
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
