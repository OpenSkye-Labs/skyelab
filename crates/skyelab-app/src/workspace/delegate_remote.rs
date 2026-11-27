//! This machine's side of delegation from the user's other machines (`delegate`, protocol 1.8). Every call needs
//! a link the user approved here for the caller's (node, device key, account), and both machines' accounts signed
//! in to the same Claude login now. A start runs under this machine's own binding for the folder, through the same
//! `ThreadView` launch a local delegated start takes, with the caller's caps claimed under the store lock. A caller
//! reaches only the threads it started. No error names a local path.

use super::thread::ThreadView;
use super::{Workspace, lock};
use crate::remote::Reply;
use gpui::{App, AppContext as _, Context, Global, WeakEntity, Window};
use serde_json::{Value, json};
use skyelab_core::delegation_remote::{self as rules, DelegationLink, Login, Refusal};
use skyelab_core::{AppSettings, Provider};
use skyelab_daemon::Origin;
use skyelab_protocol::{DelegateOp, Delegator, ErrorCode, RpcError};
use skyelab_store::{DelegatedOrigin, Thread};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Starts made here, by (origin node, origin store, parent, origin account, request id), while their tab is open.
#[derive(Default)]
pub(super) struct RemoteStarts(
    HashMap<(String, String, i64, String, String), WeakEntity<ThreadView>>,
);

impl Global for RemoteStarts {}

/// A call whose link was found and whose logins matched.
struct Linked {
    origin: Origin,
    caller: Delegator,
    link: DelegationLink,
}

fn not_allowed(why: impl Into<String>) -> RpcError {
    RpcError::new(ErrorCode::DelegationNotAllowed, why.into())
}

fn bad(why: impl Into<String>) -> RpcError {
    RpcError::new(ErrorCode::BadRequest, why.into())
}

fn unreachable(id: i64) -> RpcError {
    RpcError::new(
        ErrorCode::NotFound,
        format!("no thread {id} you started on this machine"),
    )
}

fn refusal(r: Refusal) -> RpcError {
    match r {
        Refusal::NotAllowed(m) => not_allowed(m),
        Refusal::BadRequest(m) => bad(m),
    }
}

fn wire_login(l: &skyelab_protocol::Login) -> Option<Login> {
    (l.provider == "claude").then(|| Login {
        provider: Provider::Claude,
        email: l.email.to_ascii_lowercase(),
        org_id: l.org_id.clone(),
    })
}

impl Workspace {
    /// Runs one `delegate` call: link and logins first (the login read happens off the UI thread), then the op.
    pub(super) fn remote_delegate(
        &mut self,
        origin: Origin,
        caller: Delegator,
        op: DelegateOp,
        (reply, gate): (Reply<Value>, skyelab_daemon::CallGate),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let link = match self.delegation_link(&origin, &caller, cx) {
            Ok(link) => link,
            Err(e) => {
                let _ = reply.send(Err(e));
                return;
            }
        };
        let Some(account) = self
            .saved_config(cx)
            .accounts
            .iter()
            .find(|a| a.id == link.local_account)
            .cloned()
        else {
            {
                let _ = reply.send(Err(not_allowed(
                    "the account that delegation link names isn't on this machine any more",
                )));
                return;
            }
        };
        let bin = self.backend.claude_bin.clone();
        let caller_login = wire_login(&caller.login);
        let checked_account = account.clone();
        let local =
            cx.background_spawn(async move { crate::login_cache::login(&bin, &checked_account) });
        cx.spawn_in(window, async move |this, cx| {
            let local = local.await;
            let checked = rules::check_logins(&link, caller_login.as_ref(), local.as_ref());
            let _ = this.update_in(cx, |this, window, cx| match checked {
                Err(why) => _ = reply.send(Err(refusal(why))),
                // Everything that could have changed while the login was read is checked again.
                Ok(()) if gate.cancelled() || reply.is_disconnected() => {}
                Ok(()) if this.sharing_session(cx).is_none() => {
                    let _ = reply.send(Err(bad("sharing was turned off on this machine")));
                }
                Ok(())
                    if this.delegation_link(&origin, &caller, cx).ok().as_ref() != Some(&link)
                        || !this
                            .saved_config(cx)
                            .accounts
                            .iter()
                            .any(|a| a.same_login(&account)) =>
                {
                    let _ = reply.send(Err(not_allowed(
                        "this machine's delegation link or account changed while the call was checked; call again",
                    )));
                }
                Ok(()) => {
                    let linked = Linked {
                        origin,
                        caller,
                        link,
                    };
                    this.delegate_linked(linked, op, reply, window, cx)
                }
            });
        })
        .detach();
    }

    /// accounts.toml as saved now: a rebind may not have reached this window's copy yet.
    fn saved_config(&self, cx: &App) -> skyelab_core::AccountsConfig {
        let file = cx.try_global::<super::AccountsFile>().map(|f| f.0.clone());
        super::accounts_now(file.as_deref(), &self.config)
    }

    /// The link for this caller, read from settings.toml as it is now.
    fn delegation_link(
        &self,
        origin: &Origin,
        caller: &Delegator,
        cx: &App,
    ) -> Result<DelegationLink, RpcError> {
        let path = self
            .sharing()
            .and_then(|r| r.read(cx).settings_path().map(std::path::Path::to_path_buf))
            .ok_or_else(|| {
                not_allowed("this machine keeps no settings, so it allows no delegation")
            })?;
        let settings = AppSettings::load(&path).map_err(|_| {
            not_allowed("this machine's settings can't be read, so it allows no delegation")
        })?;
        rules::link_for(
            &settings.delegation.links,
            &origin.node_id,
            &origin.key_hash,
            &caller.account_id,
        )
        .cloned()
        .ok_or_else(|| {
            not_allowed("this machine hasn't allowed delegation from that machine and account")
        })
    }

    fn delegate_linked(
        &mut self,
        l: Linked,
        op: DelegateOp,
        reply: Reply<Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answered = match op {
            DelegateOp::ListProjects => Ok(Value::Array({
                let config = self.saved_config(cx);
                self.projects
                    .iter()
                    .filter(|p| rules::check_remote_target(&config, &l.link, &p.path).is_ok())
                    .map(|p| json!({"id": p.id, "name": p.name, "path": p.path}))
                    .collect()
            })),
            DelegateOp::Start {
                project_id,
                prompt,
                worktree,
                request_id,
            } => {
                let start = (project_id, prompt.as_str(), worktree, request_id.as_str());
                return self.remote_child_start(&l, start, reply, window, cx);
            }
            DelegateOp::Status { thread_id } => self.reachable(&l, thread_id, cx).map(|t| {
                json!({"thread_id": t.id, "title": t.title, "status": t.status, "project_id": t.project_id,
                    "account_id": t.account_id})
            }),
            DelegateOp::Read {
                thread_id,
                since_seq,
            } => self.reachable(&l, thread_id, cx).and_then(|t| {
                super::orchestrate::read(&self.backend.store, &t, since_seq).map_err(bad)
            }),
            DelegateOp::Wait {
                thread_id,
                timeout_s,
            } => match self.reachable(&l, thread_id, cx) {
                Ok(_) => return self.remote_wait(thread_id, timeout_s, reply, cx),
                Err(e) => Err(e),
            },
            DelegateOp::Send { thread_id, text } => match self.reachable(&l, thread_id, cx) {
                Ok(_) => return self.remote_child_send(thread_id, &text, reply, window, cx),
                Err(e) => Err(e),
            },
            DelegateOp::Interrupt { thread_id } => match self.reachable(&l, thread_id, cx) {
                // Still starting: stop it, so its agent ends as soon as it is up.
                Ok(_) if self.open_view(thread_id, cx).is_some_and(|v| v.read(cx).starting) => {
                    if let Some(view) = self.open_view(thread_id, cx) {
                        view.update(cx, |v, cx| v.stop(&super::thread::Stop, window, cx));
                    }
                    Ok(json!({"ok": true}))
                }
                Ok(_) => {
                    let (tx, rx) = flume::bounded(1);
                    self.remote_interrupt(thread_id, tx, cx);
                    return forward(rx, reply, |()| json!({"ok": true}), cx);
                }
                Err(e) => Err(e),
            },
        };
        let _ = reply.send(answered);
    }

    /// Thread `id` if this caller started it: the same origin node, store and parent thread, the same account
    /// there, and still under the link's account here.
    fn reachable(&self, l: &Linked, id: i64, cx: &App) -> Result<Thread, RpcError> {
        let (thread, from) = lock(&self.backend.store)
            .and_then(|s| Ok((s.thread(id)?, s.delegated_from(id)?)))
            .map_err(|_| unreachable(id))?;
        let (Some(thread), Some(from)) = (thread, from) else {
            return Err(unreachable(id));
        };
        let mine = from.origin_node == l.origin.node_id
            && from.origin_store == l.caller.store_id
            && from.parent_thread_id == l.caller.thread_id
            && from.origin_account == l.caller.account_id
            && thread.account_id == l.link.local_account;
        if !mine {
            return Err(unreachable(id));
        }
        let project = self
            .projects
            .iter()
            .find(|p| p.id == thread.project_id)
            .ok_or_else(|| unreachable(id))?;
        rules::check_remote_target(&self.saved_config(cx), &l.link, &project.path)
            .map_err(refusal)?;
        Ok(thread)
    }

    fn remote_child_start(
        &mut self,
        l: &Linked,
        (project_id, prompt, worktree, request_id): (i64, &str, bool, &str),
        reply: Reply<Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let started = (|| {
            let depth = rules::remote_child_depth(l.caller.depth).map_err(bad)?;
            let key = (
                l.origin.node_id.clone(),
                l.caller.store_id.clone(),
                l.caller.thread_id,
                l.caller.account_id.clone(),
                request_id.to_owned(),
            );
            // A retry: the same thread, whether it is still starting or was saved before a restart.
            let pending = cx
                .try_global::<RemoteStarts>()
                .and_then(|s| s.0.get(&key))
                .and_then(WeakEntity::upgrade)
                // One that failed before its row starts afresh.
                .filter(|v| {
                    super::orchestrate_ui::row_result(v.read(cx)).is_none_or(|r| r.is_ok())
                });
            if let Some(view) = pending {
                return Ok(Err(view));
            }
            let stored = lock(&self.backend.store)
                .and_then(|s| s.delegated_by_request((&key.0, &key.1, key.2, &key.3), &key.4))
                .map_err(|_| RpcError::new(ErrorCode::Internal, "store error"))?;
            if let Some(id) = stored {
                return Ok(Ok(id));
            }
            let project = self
                .projects
                .iter()
                .find(|p| p.id == project_id)
                .cloned()
                .ok_or_else(|| bad(format!("no project {project_id}; call list_projects")))?;
            let account =
                rules::check_remote_target(&self.saved_config(cx), &l.link, &project.path)
                    .map_err(refusal)?
                    .id
                    .clone();
            let origin = DelegatedOrigin {
                origin_node: key.0.clone(),
                origin_store: key.1.clone(),
                origin_machine: l.origin.device.clone(),
                parent_thread_id: key.2,
                parent_title: rules::clean_title(&l.caller.title),
                origin_account: l.caller.account_id.clone(),
                depth,
                request_id: Some(key.4.clone()),
            };
            // Refused here so a full caller gets no tab; the claim inside the launch, under the store lock, decides.
            lock(&self.backend.store)
                .and_then(|s| {
                    super::child_slots::claim_for(
                        &s,
                        &super::child_slots::remote_key(&origin),
                        None,
                    )
                })
                .map_err(|e| bad(format!("{e:#}")))?;
            let (config, backend) = (self.config.clone(), self.backend.clone());
            let view = cx.new(|cx| {
                let mut v = ThreadView::new(&project, config, backend, window, cx);
                v.remote_parent = Some(origin);
                v.wt.use_worktree = worktree;
                // Never Full access: Ask, or Plan when that is the project's default.
                if v.mode != skyelab_agent::Mode::Plan {
                    v.mode = skyelab_agent::Mode::Default;
                }
                v.required_account = Some(account);
                v
            });
            view.update(cx, |v, cx| v.submit_remote(prompt, window, cx))
                .map_err(bad)?;
            self.add_thread_tab(view.clone(), false, window, cx);
            let starts = &mut cx.default_global::<RemoteStarts>().0;
            starts.retain(|_, v| v.upgrade().is_some());
            starts.insert(key, view.downgrade());
            Ok(Err(view))
        })();
        match started {
            Err(e) => _ = reply.send(Err(e)),
            Ok(Ok(id)) => _ = reply.send(Ok(json!({"thread_id": id}))),
            Ok(Err(view)) => {
                let (tx, rx) = flume::bounded(1);
                super::orchestrate_ui::reply_when_row(&view, None, tx, cx);
                let rx_mapped = async move {
                    match rx.recv_async().await {
                        Ok(Ok(id)) => Ok(json!({"thread_id": id})),
                        Ok(Err(why)) => Err(bad(why)),
                        Err(_) => Err(bad("the thread's tab closed")),
                    }
                };
                cx.spawn(async move |_, _| _ = reply.send(rx_mapped.await))
                    .detach();
            }
        }
    }

    /// A follow-up to a child that is still running here; answered once its store write (and slot claim) ran.
    fn remote_child_send(
        &mut self,
        thread_id: i64,
        text: &str,
        reply: Reply<Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.open_view(thread_id, cx) else {
            {
                let _ = reply.send(Err(bad(format!(
                    "thread {thread_id} isn't running here (it ended, or its tab was closed)"
                ))));
                return;
            }
        };
        let (starting, running) = {
            let v = view.read(cx);
            (v.starting, v.session.is_some())
        };
        if starting || !running {
            {
                let _ = reply.send(Err(bad(format!(
                "thread {thread_id} isn't running (it is starting, ended or was stopped); check thread_status"
            ))));
                return;
            }
        }
        let (tx, rx) = flume::bounded(1);
        view.update(cx, |v, cx| {
            v.on_sent = Some(tx);
            let taken = v.submit_remote(text, window, cx);
            if let Some(tx) = v.on_sent.take() {
                let _ = tx.send(taken);
            }
        });
        let rx = async move {
            match rx.recv_async().await {
                Ok(Ok(())) => Ok(json!({"ok": true})),
                Ok(Err(why)) => Err(bad(why)),
                Err(_) => Err(bad("the message was dropped")),
            }
        };
        cx.spawn(async move |_, _| _ = reply.send(rx.await))
            .detach();
    }

    /// One bounded wait round: done once the thread isn't working on a turn, else its status at the deadline.
    fn remote_wait(
        &mut self,
        thread_id: i64,
        timeout_s: u64,
        reply: Reply<Value>,
        cx: &mut Context<Self>,
    ) {
        let store = self.backend.store.clone();
        let timer = cx.background_executor().clone();
        let deadline = Instant::now() + Duration::from_secs(timeout_s.min(rules::MAX_WAIT_S));
        cx.background_spawn(async move {
            loop {
                let row = lock(&store).and_then(|s| s.thread(thread_id));
                let Ok(Some(t)) = row else {
                    {
                        let _ = reply.send(Err(unreachable(thread_id)));
                        return;
                    }
                };
                let done = !skyelab_core::delegation::is_busy(&t.status);
                if done || Instant::now() >= deadline || reply.is_disconnected() {
                    let mut out = json!({"thread_id": t.id, "status": t.status, "done": done});
                    if done && let Ok(Some(turn)) = super::orchestrate::last_turn(&store, t.id) {
                        out["turn"] = json!(turn);
                    }
                    {
                        let _ = reply.send(Ok(out));
                        return;
                    }
                }
                timer.timer(Duration::from_millis(250)).await;
            }
        })
        .detach();
    }
}

/// Sends `rx`'s answer to `reply`, mapped to JSON.
fn forward<T: Send + 'static>(
    rx: flume::Receiver<Result<T, RpcError>>,
    reply: Reply<Value>,
    map: impl FnOnce(T) -> Value + 'static,
    cx: &mut Context<Workspace>,
) {
    cx.spawn(async move |_, _| {
        let answer = match rx.recv_async().await {
            Ok(r) => r.map(map),
            Err(_) => Err(RpcError::new(ErrorCode::Internal, "no answer")),
        };
        let _ = reply.send(answer);
    })
    .detach();
}
