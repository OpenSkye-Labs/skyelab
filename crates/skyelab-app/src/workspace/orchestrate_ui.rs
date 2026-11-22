//! The Workspace answers delegated starts and sends on the UI thread, through the same `ThreadView` path a
//! local prompt takes. The account rule is checked again here, against the config the launch will use.

use super::Workspace;
use super::orchestrate::{OrchCall, Reply, StartRequest};
use super::thread::{ChildLink, ThreadView};
use gpui::{AppContext as _, Context, SharedString, Window};
use skyelab_agent::mcp::Caller;
use skyelab_core::delegation::check_limits;

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
                    return; // window closed; dropping `call` tells the agent SkyeLab is closing
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
                if let Err(why) = self.delegate(&request, reply.clone(), window, cx) {
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
                let _ = reply.send(self.delegated_send(&caller, thread_id, &text, window, cx));
            }
        }
    }

    /// Live (starting or running) children of `parent`, from the open tabs.
    fn live_children(&self, parent: i64, cx: &Context<Self>) -> usize {
        self.open_threads
            .iter()
            .filter_map(|t| t.upgrade())
            .filter(|t| {
                let t = t.read(cx);
                t.parent.as_ref().is_some_and(|p| p.0 == parent)
                    && (t.starting || t.session.is_some())
            })
            .count()
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
        } = request;
        let project = self
            .projects
            .iter()
            .find(|p| p.id == *project_id)
            .cloned()
            .ok_or_else(|| format!("no project {project_id}; call list_projects"))?;
        self.config
            .delegation_account(&caller.account_id, &project.path)?;
        check_limits(*depth, self.live_children(caller.thread_id, cx))?;
        let parent_title: SharedString = self.open_view(caller.thread_id, cx).map_or_else(
            || format!("thread {}", caller.thread_id).into(),
            |v| v.read(cx).title(),
        );
        let (config, backend) = (self.config.clone(), self.backend.clone());
        let view = cx.new(|cx| {
            let mut v = ThreadView::new(&project, config, backend, window, cx);
            v.parent = Some((caller.thread_id, parent_title));
            v.wt.use_worktree = *worktree;
            v
        });
        view.update(cx, |v, cx| v.submit_remote(prompt, window, cx))?;
        self.add_thread_tab(view.clone(), false, window, cx);
        self.reply_when_started(&view, move |r| _ = reply.send(r.map_err(str::to_owned)), cx);
        Ok(())
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
        let thread = view
            .read(cx)
            .thread
            .clone()
            .ok_or("that thread is still starting")?;
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
