//! The calling side of delegation across machines: an agent's tool call with a `machine` goes to that linked
//! machine's `delegate` (protocol 1.8). The other machine checks everything again with its own data.

use super::{AppHost, CALLER_GONE, OrchCall, Reply, ask};
use crate::workspace::child_slots::busy;
use serde_json::{Value, json};
use skyelab_agent::mcp::{Caller, Tool};
use skyelab_client::{Error, Pending};
use skyelab_core::delegation::{check_children, check_depth, is_remote_busy};
use skyelab_core::delegation_remote::{MAX_WAIT_S, clean_title};
use skyelab_protocol::{DelegateOp, Delegator, ErrorCode};
use skyelab_store::{RemoteChild, Thread};
use std::time::{Duration, Instant};

/// How long a start keeps trying to reach a machine whose connection dropped before it answered.
const RECONNECT: Duration = Duration::from_secs(30);
/// Longer than the other machine's own 30 s wait for its UI, so its answer (or timeout) arrives first.
const ANSWER: Duration = Duration::from_secs(40);
/// A remote child the other machine no longer has.
const GONE: &str = "gone";

/// The UI thread's answer: the call went out on the linked connection (with the node it presented), or the
/// machine isn't connected and nothing was sent.
pub enum Sent {
    Out(Pending<Value>, String),
    Offline(String),
}

pub struct RemoteCall {
    caller: Caller,
    machine: String,
    delegator: Delegator,
    op: DelegateOp,
    /// The node a known child was started on: refused if another node answers to the name now.
    node: Option<String>,
    reply: Reply<Sent>,
}

impl RemoteCall {
    pub(in crate::workspace) fn is_abandoned(&self) -> bool {
        self.reply.is_disconnected()
    }
}

/// Why a call got no answer.
enum Failed {
    /// Nothing was sent: the machine isn't connected.
    Offline(String),
    /// It may have reached the other machine, which didn't answer.
    Lost(String),
    Refused(Option<ErrorCode>, String),
}

impl Failed {
    fn text(self) -> String {
        match self {
            Failed::Offline(m) | Failed::Lost(m) | Failed::Refused(_, m) => m,
        }
    }
}

impl AppHost {
    pub(super) fn machines(&self, caller: &Caller) -> Result<Value, String> {
        let mut list = ask(&self.calls, |reply| OrchCall::Machines { reply })?;
        let refused = self
            .not_linked
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default();
        for m in &mut list {
            let host = m["machine"].as_str().unwrap_or_default().to_owned();
            if m["state"] == "ready" && refused.contains(&(host, caller.account_id.clone())) {
                m["state"] = json!("not_linked_here");
                m["reason"] =
                    json!("it refused this machine and account; the user can allow it there");
            }
        }
        Ok(Value::Array(list))
    }

    fn note_linked(&self, machine: &str, caller: &Caller, linked: bool) {
        if let Ok(mut s) = self.not_linked.lock() {
            let key = (machine.to_owned(), caller.account_id.clone());
            match linked {
                true => s.remove(&key),
                false => s.insert(key),
            };
        }
    }

    /// One `delegate` call to `machine`, as `me`, on the node `node` when a known child's.
    fn send(
        &self,
        caller: &Caller,
        me: &Thread,
        (machine, node): (&str, Option<&str>),
        op: DelegateOp,
    ) -> Result<(Value, String), Failed> {
        let refused = |e: String| Failed::Refused(None, e);
        let delegator = Delegator {
            store_id: self.store(|s| s.store_id()).map_err(refused)?,
            thread_id: me.id,
            title: clean_title(&me.title),
            account_id: caller.account_id.clone(),
            login: Default::default(), // filled on the UI thread, which knows the agent binary
            depth: self.depth(me).map_err(refused)?,
        };
        let call = RemoteCall {
            caller: caller.clone(),
            machine: machine.to_owned(),
            delegator,
            op,
            node: node.map(str::to_owned),
            reply: flume::bounded(1).0,
        };
        let (pending, node) = match ask(&self.calls, |reply| {
            OrchCall::Remote(Box::new(RemoteCall { reply, ..call }))
        }) {
            Ok(Sent::Out(pending, node)) => (pending, node),
            Ok(Sent::Offline(why)) => return Err(Failed::Offline(why)),
            Err(why) => return Err(refused(why)),
        };
        match pending.recv_timeout(ANSWER) {
            Ok(v) => {
                self.note_linked(machine, caller, true);
                Ok((v, node))
            }
            Err(Error::Rpc(e)) if e.code == ErrorCode::DelegationNotAllowed => {
                self.note_linked(machine, caller, false);
                let why = format!(
                    "{machine} doesn't allow this: {}. The user is asked there (Settings → Sharing & access).",
                    e.message
                );
                Err(Failed::Refused(Some(e.code), why))
            }
            // The other machine's UI didn't answer in time; its launch may still go on.
            Err(Error::Rpc(e)) if e.code == ErrorCode::Internal => Err(Failed::Lost(format!(
                "{machine} didn't finish answering ({})",
                e.message
            ))),
            Err(Error::Rpc(e)) => Err(Failed::Refused(
                Some(e.code),
                format!("{machine}: {}", e.message),
            )),
            Err(Error::NotConnected) => Err(Failed::Offline(format!(
                "{machine} isn't connected; nothing was sent"
            ))),
            Err(e) => Err(Failed::Lost(format!(
                "the connection to {machine} dropped before it answered ({e})"
            ))),
        }
    }

    /// The child `me` started on `machine` as `thread_id`, or why it can't be reached. No network.
    fn child(&self, me: &Thread, machine: &str, thread_id: i64) -> Result<RemoteChild, String> {
        self.store(|s| s.remote_children(me.id))?
            .into_iter()
            .find(|c| c.machine == machine && c.thread_id == thread_id)
            .ok_or_else(|| {
                format!("thread {thread_id} on {machine} wasn't started by this thread; you can only reach threads you started")
            })
    }

    /// Sends `op` about child `c` and keeps its last known status: what it answered, "" when unreachable.
    fn send_child(
        &self,
        caller: &Caller,
        me: &Thread,
        c: &RemoteChild,
        op: DelegateOp,
    ) -> Result<Value, Failed> {
        let sent = self.send(caller, me, (&c.machine, Some(&c.node_id)), op);
        let status = match &sent {
            Ok((v, _)) => v["status"].as_str().map(str::to_owned),
            Err(Failed::Refused(Some(ErrorCode::NotFound), _)) => Some(GONE.to_owned()),
            Err(Failed::Offline(_) | Failed::Lost(_)) => Some(String::new()),
            Err(Failed::Refused(..)) => None,
        };
        if let Some(status) = status {
            let _ =
                self.store(|s| s.set_remote_child_status(me.id, &c.node_id, c.thread_id, &status));
        }
        sent.map(|(v, _)| v)
    }

    pub(super) fn remote(
        &self,
        caller: &Caller,
        me: &Thread,
        machine: &str,
        tool: Tool,
        granted: &dyn Fn() -> bool,
    ) -> Result<Value, String> {
        let (thread_id, op) = match tool {
            Tool::ListProjects => {
                let sent = self.send(caller, me, (machine, None), DelegateOp::ListProjects);
                return sent.map(|(v, _)| v).map_err(Failed::text);
            }
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
            Tool::WaitThread {
                thread_id,
                timeout_s,
            } => {
                let c = self.child(me, machine, thread_id)?;
                return self.remote_wait(caller, me, &c, timeout_s, granted);
            }
            Tool::ThreadStatus { thread_id } => (thread_id, DelegateOp::Status { thread_id }),
            Tool::ReadThread {
                thread_id,
                since_seq,
            } => (
                thread_id,
                DelegateOp::Read {
                    thread_id,
                    since_seq,
                },
            ),
            Tool::SendMessage { thread_id, text } => {
                let c = self.child(me, machine, thread_id)?;
                // A new turn for an idle child takes one of this parent's slots, wherever the child runs.
                if !is_remote_busy(&c.status) && c.status != GONE {
                    check_children(self.store(|s| busy(s, me.id, None))?)?;
                }
                return match self.send_child(caller, me, &c, DelegateOp::Send { thread_id, text }) {
                    Ok(v) => {
                        let _ = self.store(|s| {
                            s.set_remote_child_status(me.id, &c.node_id, thread_id, "running")
                        });
                        Ok(v)
                    }
                    Err(Failed::Lost(e)) => Err(format!(
                        "{e}; the message may or may not have been delivered. Check read_thread before sending again"
                    )),
                    Err(e) => Err(e.text()),
                };
            }
            _ => return Err("that tool can't run on another machine".into()),
        };
        let c = self.child(me, machine, thread_id)?;
        self.send_child(caller, me, &c, op).map_err(Failed::text)
    }

    /// Busy children of `me` here and on every machine, the remote ones' statuses read again first. One whose
    /// status can't be read counts as busy.
    fn busy_children(&self, caller: &Caller, me: &Thread) -> Result<usize, String> {
        for c in self.store(|s| s.remote_children(me.id))? {
            if is_remote_busy(&c.status) {
                let _ = self.send_child(
                    caller,
                    me,
                    &c,
                    DelegateOp::Status {
                        thread_id: c.thread_id,
                    },
                );
            }
        }
        self.store(|s| busy(s, me.id, None))
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
            match self.send(caller, me, (machine, None), op()) {
                Ok(answer) => break answer,
                Err(Failed::Offline(e)) if tries == 1 => return Err(e),
                Err(Failed::Offline(_) | Failed::Lost(_))
                    if began.elapsed() < RECONNECT && granted() =>
                {
                    std::thread::sleep(Duration::from_millis(500));
                }
                Err(e @ (Failed::Offline(_) | Failed::Lost(_))) => {
                    return Err(format!(
                        "{}, so it isn't known whether the thread started; call start_thread again with machine \
                         {machine} and request_id \"{request_id}\" to find out",
                        e.text()
                    ));
                }
                Err(e) => return Err(e.text()),
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
            status: "starting".into(),
            created_at: 0,
        };
        if let Err(e) = self.store(|s| s.add_remote_child(&child)) {
            // A child nobody here can see or reach must not keep running there.
            let stopped = self.send(
                caller,
                me,
                (machine, Some(&child.node_id)),
                DelegateOp::Interrupt { thread_id },
            );
            let how = if stopped.is_ok() {
                "it was stopped there"
            } else {
                "stopping it there failed too"
            };
            return Err(format!(
                "thread {thread_id} started on {machine} but couldn't be recorded here ({e}); {how}"
            ));
        }
        let _ = self.calls.send(OrchCall::ChildRecorded { parent: me.id });
        Ok(json!({"thread_id": thread_id, "machine": machine}))
    }

    fn remote_wait(
        &self,
        caller: &Caller,
        me: &Thread,
        c: &RemoteChild,
        timeout_s: u64,
        granted: &dyn Fn() -> bool,
    ) -> Result<Value, String> {
        let deadline = Instant::now() + Duration::from_secs(timeout_s);
        loop {
            if !granted() {
                return Err(CALLER_GONE.into());
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let op = DelegateOp::Wait {
                thread_id: c.thread_id,
                timeout_s: left.as_secs().clamp(1, MAX_WAIT_S),
            };
            match self.send_child(caller, me, c, op) {
                Ok(v) if v["done"] == true || Instant::now() >= deadline => return Ok(v),
                Ok(_) => {}
                Err(Failed::Offline(_) | Failed::Lost(_)) if Instant::now() >= deadline => {
                    return Ok(json!({
                        "thread_id": c.thread_id, "done": false, "status": "unreachable",
                        "note": format!("{} can't be reached right now; the thread keeps running there", c.machine),
                    }));
                }
                Err(Failed::Offline(_) | Failed::Lost(_)) => {
                    std::thread::sleep(Duration::from_millis(500))
                }
                Err(e) => return Err(e.text()),
            }
        }
    }
}

#[path = "delegate_out_ui.rs"]
mod ui;
