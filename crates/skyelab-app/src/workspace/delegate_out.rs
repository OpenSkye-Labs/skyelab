//! The calling side of delegation across machines: an agent's tool call with a `machine` goes to that linked
//! machine's `delegate` (protocol 1.8). This machine checks first, the other checks again with its own data: the
//! target must have "Let my agents delegate here" on, be linked, connected and speak 1.8; a thread call reaches only
//! a child this thread started there (`remote_children`); a start counts remote children with local ones against
//! the cap (one whose status can't be read counts as busy) and records the child, or stops it if it can't.

use super::{AppHost, CALLER_GONE, OrchCall, Reply, ask};
use crate::workspace::Workspace;
use gpui::{AppContext as _, Context, Window};
use serde_json::{Value, json};
use skyelab_agent::mcp::{Caller, Tool};
use skyelab_client::{Error, Pending};
use skyelab_core::AppSettings;
use skyelab_core::delegation::{check_children, check_depth};
use skyelab_protocol::{DelegateOp, Delegator, ErrorCode};
use skyelab_store::{RemoteChild, Thread};
use std::time::{Duration, Instant};

/// How long a start or send keeps trying to reach a machine whose connection dropped mid-call.
const RECONNECT: Duration = Duration::from_secs(30);
/// One wait round on the other machine.
const ROUND_S: u64 = 20;
/// A call's answer, on top of the other machine's own 30 s.
const ANSWER: Duration = Duration::from_secs(40);

/// What goes to the UI thread: the call, and the caller whose login it proves.
pub struct RemoteCall {
    caller: Caller,
    machine: String,
    delegator: Delegator,
    op: DelegateOp,
    /// The pending answer, and the node the connection presented.
    reply: Reply<(Pending<Value>, String)>,
}

impl RemoteCall {
    pub(in crate::workspace) fn is_abandoned(&self) -> bool {
        self.reply.is_disconnected()
    }
}

/// A call that can't have reached the other machine, or whose connection dropped before it answered.
fn unreachable(e: &str) -> bool {
    e.contains("isn't connected") || e.contains("dropped") || e.contains("not connected")
}

impl AppHost {
    pub(super) fn machines(&self) -> Result<Value, String> {
        let mut list = ask(&self.calls, |reply| OrchCall::Machines { reply })?;
        let refused = self
            .not_linked
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default();
        for m in &mut list {
            if m["state"] == "ready" && m["machine"].as_str().is_some_and(|h| refused.contains(h)) {
                m["state"] = json!("not_linked_here");
                m["reason"] =
                    json!("it refused this machine and account; the user can allow it there");
            }
        }
        Ok(Value::Array(list))
    }

    /// One `delegate` call to `machine`, as `me`. Errors are the agent's words.
    fn send(
        &self,
        caller: &Caller,
        me: &Thread,
        machine: &str,
        op: DelegateOp,
    ) -> Result<(Value, String), String> {
        let delegator = Delegator {
            store_id: self.store(|s| s.store_id())?,
            thread_id: me.id,
            title: me.title.clone(),
            account_id: caller.account_id.clone(),
            login: Default::default(), // filled on the UI thread, which knows the agent binary
            depth: self.depth(me)?,
        };
        let call = RemoteCall {
            caller: caller.clone(),
            machine: machine.to_owned(),
            delegator,
            op,
            reply: flume::bounded(1).0,
        };
        let (pending, node) = ask(&self.calls, |reply| {
            OrchCall::Remote(RemoteCall { reply, ..call })
        })?;
        match pending.recv_timeout(ANSWER) {
            Ok(v) => {
                if let Ok(mut s) = self.not_linked.lock() {
                    s.remove(machine);
                }
                Ok((v, node))
            }
            Err(Error::Rpc(e)) if e.code == ErrorCode::DelegationNotAllowed => {
                if let Ok(mut s) = self.not_linked.lock() {
                    s.insert(machine.to_owned());
                }
                Err(format!(
                    "{machine} doesn't allow this: {}. The user is asked there (Settings → Sharing & access).",
                    e.message
                ))
            }
            Err(Error::Rpc(e)) => Err(format!("{machine}: {}", e.message)),
            Err(Error::ConnectionLost | Error::Timeout) => Err(format!(
                "the connection to {machine} dropped before it answered"
            )),
            Err(e) => Err(format!("{machine} isn't connected ({e}); nothing was sent")),
        }
    }

    /// Fails unless `thread_id` is a child `me` started on `machine`. No network.
    fn child(&self, me: &Thread, machine: &str, thread_id: i64) -> Result<RemoteChild, String> {
        self.store(|s| s.remote_children(me.id))?
            .into_iter()
            .find(|c| c.machine == machine && c.thread_id == thread_id)
            .ok_or_else(|| {
                format!("thread {thread_id} on {machine} wasn't started by this thread; you can only reach threads you started")
            })
    }

    pub(super) fn remote(
        &self,
        caller: &Caller,
        me: &Thread,
        machine: &str,
        tool: Tool,
        granted: &dyn Fn() -> bool,
    ) -> Result<Value, String> {
        let op = match tool {
            Tool::ListProjects => DelegateOp::ListProjects,
            Tool::StartThread {
                project_id,
                prompt,
                worktree,
                request_id,
            } => {
                return self.remote_start(
                    caller,
                    me,
                    machine,
                    (project_id, prompt, worktree, request_id),
                    granted,
                );
            }
            Tool::ThreadStatus { thread_id } => {
                self.child(me, machine, thread_id)?;
                DelegateOp::Status { thread_id }
            }
            Tool::ReadThread {
                thread_id,
                since_seq,
            } => {
                self.child(me, machine, thread_id)?;
                DelegateOp::Read {
                    thread_id,
                    since_seq,
                }
            }
            Tool::SendMessage { thread_id, text } => {
                self.child(me, machine, thread_id)?;
                return match self.send(caller, me, machine, DelegateOp::Send { thread_id, text }) {
                    Err(e) if e.contains("dropped") => Err(format!(
                        "{e}; the message may or may not have been delivered. Check read_thread before sending again"
                    )),
                    other => other.map(|(v, _)| v),
                };
            }
            Tool::WaitThread {
                thread_id,
                timeout_s,
            } => {
                self.child(me, machine, thread_id)?;
                return self.remote_wait(caller, me, machine, thread_id, timeout_s, granted);
            }
            _ => return Err("that tool can't run on another machine".into()),
        };
        self.send(caller, me, machine, op).map(|(v, _)| v)
    }

    /// Busy children of `me` here and on every machine: a remote one whose status can't be read counts as busy.
    fn busy_children(&self, caller: &Caller, me: &Thread) -> Result<usize, String> {
        let local = self.store(|s| crate::workspace::child_slots::busy(s, me.id, None))?;
        let remote = self.store(|s| s.remote_children(me.id))?;
        let busy = remote
            .iter()
            .filter(|c| {
                let status = self.send(
                    caller,
                    me,
                    &c.machine,
                    DelegateOp::Status {
                        thread_id: c.thread_id,
                    },
                );
                status.map_or(true, |(v, _)| {
                    v["status"]
                        .as_str()
                        .is_none_or(skyelab_core::delegation::is_busy)
                })
            })
            .count();
        Ok(local + busy)
    }

    fn remote_start(
        &self,
        caller: &Caller,
        me: &Thread,
        machine: &str,
        (project_id, prompt, worktree, request_id): (i64, String, bool, Option<String>),
        granted: &dyn Fn() -> bool,
    ) -> Result<Value, String> {
        check_depth(self.depth(me)?)?;
        check_children(self.busy_children(caller, me)?)?;
        // Always one, so this machine's own retry after a dropped connection can't start twice.
        let request_id = match request_id {
            Some(r) => r,
            None => skyelab_protocol::new_device_key().map_err(|e| e.to_string())?[..32].to_owned(),
        };
        let title: String = prompt.chars().take(80).collect();
        let op = || DelegateOp::Start {
            project_id,
            prompt: prompt.clone(),
            worktree,
            request_id: request_id.clone(),
        };
        let began = Instant::now();
        let mut tries = 0;
        let (reply, node) = loop {
            tries += 1;
            match self.send(caller, me, machine, op()) {
                Ok(answer) => break answer,
                // Not connected at the first try: nothing was sent, so say so at once.
                Err(e) if e.contains("isn't connected") && tries == 1 => return Err(e),
                Err(e) if unreachable(&e) && began.elapsed() < RECONNECT && granted() => {
                    std::thread::sleep(Duration::from_millis(500));
                }
                Err(e) if unreachable(&e) => {
                    return Err(format!(
                        "{e}, so it isn't known whether the thread started; call start_thread again with \
                         machine {machine} and request_id \"{request_id}\" to find out"
                    ));
                }
                Err(e) => return Err(e),
            }
        };
        let thread_id = reply["thread_id"]
            .as_i64()
            .ok_or_else(|| format!("{machine} answered without a thread id"))?;
        let child = RemoteChild {
            parent_thread_id: me.id,
            machine: machine.to_owned(),
            node_id: node,
            thread_id,
            title,
            created_at: 0,
        };
        if let Err(e) = self.store(|s| s.add_remote_child(&child)) {
            // A child nobody here can see or reach must not keep running there.
            let stopped = self.send(caller, me, machine, DelegateOp::Interrupt { thread_id });
            let how = if stopped.is_ok() {
                "it was stopped there"
            } else {
                "stopping it there failed too"
            };
            return Err(format!(
                "thread {thread_id} started on {machine} but couldn't be recorded here ({e}); {how}"
            ));
        }
        Ok(json!({"thread_id": thread_id, "machine": machine}))
    }

    fn remote_wait(
        &self,
        caller: &Caller,
        me: &Thread,
        machine: &str,
        thread_id: i64,
        timeout_s: u64,
        granted: &dyn Fn() -> bool,
    ) -> Result<Value, String> {
        let deadline = Instant::now() + Duration::from_secs(timeout_s);
        loop {
            if !granted() {
                return Err(CALLER_GONE.into());
            }
            let left = deadline
                .saturating_duration_since(Instant::now())
                .as_secs()
                .clamp(1, ROUND_S);
            let done = Instant::now() >= deadline;
            match self.send(
                caller,
                me,
                machine,
                DelegateOp::Wait {
                    thread_id,
                    timeout_s: left,
                },
            ) {
                Ok((v, _)) if v["done"] == true || Instant::now() >= deadline => return Ok(v),
                Ok(_) => {}
                Err(e) if unreachable(&e) && done => {
                    return Ok(json!({
                        "thread_id": thread_id, "done": false, "status": "unreachable",
                        "note": format!("{machine} can't be reached right now; the thread keeps running there"),
                    }));
                }
                Err(e) if unreachable(&e) => std::thread::sleep(Duration::from_millis(500)),
                Err(e) => return Err(e),
            }
        }
    }
}

impl Workspace {
    /// Checks the target on the UI thread and sends the call on its linked connection.
    pub(in crate::workspace) fn remote_out(
        &mut self,
        mut call: RemoteCall,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let machine = call.machine.clone();
        let refuse = |call: RemoteCall, why: String| _ = call.reply.send(Err(why));
        if let Err(why) = self
            .caller_live(&call.caller, cx)
            .and_then(|()| self.target(&machine, cx).map(drop))
        {
            return refuse(call, why);
        }
        let Some(account) = self
            .config
            .accounts
            .iter()
            .find(|a| a.id == call.caller.account_id)
            .cloned()
        else {
            return refuse(call, "this thread's account isn't here any more".into());
        };
        let bin = self.backend.claude_bin.clone();
        let login = cx.background_spawn(async move { crate::login_cache::login(&bin, &account) });
        cx.spawn_in(window, async move |this, cx| {
            let login = login.await;
            let _ = this.update(cx, |this, cx| {
                let Some(login) = login else {
                    return refuse(
                        call,
                        "this thread's account has no Claude login with an email and organization \
                        that the other machine could check, so nothing was sent"
                            .into(),
                    );
                };
                call.delegator.login = skyelab_protocol::Login {
                    provider: "claude".into(),
                    email: login.email,
                    org_id: login.org_id,
                };
                match this.target(&machine, cx) {
                    Ok((client, node)) => {
                        let pending = client.delegate(call.delegator.clone(), call.op.clone());
                        let _ = call.reply.send(Ok((pending, node)));
                    }
                    Err(why) => refuse(call, why),
                }
            });
        })
        .detach();
    }

    /// The linked connection to `machine` if this machine's agents may delegate there now.
    fn target(
        &self,
        machine: &str,
        cx: &gpui::App,
    ) -> Result<(std::rc::Rc<skyelab_client::Client>, String), String> {
        let on = self
            .env
            .settings_path
            .as_deref()
            .and_then(|p| AppSettings::load(p).ok())
            .is_some_and(|s| s.is_delegation_target(machine));
        if !on {
            return Err(format!(
                "delegation to {machine} is off; the user turns on \"Let my agents delegate here\" for it in Settings → Machines"
            ));
        }
        let env = self.linked_envs_of(machine, cx).into_iter().next();
        let env = env.ok_or_else(|| format!("{machine} isn't a linked machine here"))?;
        let env = env.read(cx);
        let Some(node) = env.node().map(str::to_owned) else {
            return Err(format!(
                "{machine} isn't connected ({}); nothing was sent",
                env.state.label()
            ));
        };
        if !env.speaks(skyelab_protocol::DELEGATION_SINCE_MINOR) {
            return Err(format!(
                "{machine} runs an older SkyeLab: update SkyeLab on {machine} to delegate there"
            ));
        }
        Ok((env.client(), node))
    }

    /// The linked machines with delegation on, each `{machine, state, reason}`.
    pub(in crate::workspace) fn delegation_machines(&self, cx: &gpui::App) -> Vec<Value> {
        let settings = self
            .env
            .settings_path
            .as_deref()
            .and_then(|p| AppSettings::load(p).ok());
        let Some(settings) = settings else {
            return Vec::new();
        };
        self.env
            .linked
            .iter()
            .map(|l| l.env.read(cx))
            .filter(|e| settings.is_delegation_target(&e.host))
            .map(|e| {
                let (state, reason) =
                    match (e.node(), e.speaks(skyelab_protocol::DELEGATION_SINCE_MINOR)) {
                        (Some(_), true) => ("ready", String::new()),
                        (Some(_), false) => ("needs_update", "it runs an older SkyeLab".to_owned()),
                        (None, _) => ("offline", e.state.label().to_string()),
                    };
                json!({"machine": e.host, "state": state, "reason": reason})
            })
            .collect()
    }
}
