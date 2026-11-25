//! The daemon's agent calls, forwarded to the UI thread so they run through the thread view's own launch path.

use skyelab_daemon::{Agents, CallGate};
use skyelab_protocol::{ErrorCode, PermissionAnswer, RpcError};
use std::time::Duration;

pub type Reply<T> = flume::Sender<Result<T, RpcError>>;

/// `gate` is the daemon call's: cancelled once its device is revoked or disconnected.
pub enum RemoteCall {
    Start {
        project_id: i64,
        prompt: String,
        reply: Reply<i64>,
        gate: CallGate,
    },
    Send {
        thread_id: i64,
        text: String,
        reply: Reply<()>,
        gate: CallGate,
    },
    Interrupt {
        thread_id: i64,
        reply: Reply<()>,
        gate: CallGate,
    },
    Answer {
        thread_id: i64,
        request_id: String,
        decision: PermissionAnswer,
        reply: Reply<()>,
        gate: CallGate,
    },
    Questions {
        thread_id: i64,
        request_id: String,
        answers: Vec<Vec<String>>,
        reply: Reply<()>,
        gate: CallGate,
    },
    /// Settled, snoozed until a time, or neither (protocol 1.6).
    Shelve {
        thread_id: i64,
        settled: bool,
        snoozed_until: Option<i64>,
        reply: Reply<()>,
        gate: CallGate,
    },
}

impl RemoteCall {
    /// Its client stopped waiting (timed out, the daemon went away, or its device was revoked or disconnected).
    /// Running it now would surprise them, or let a device in after the user cut it off.
    pub fn abandoned(&self) -> bool {
        match self {
            RemoteCall::Start { reply, gate, .. } => reply.is_disconnected() || gate.cancelled(),
            RemoteCall::Send { reply, gate, .. }
            | RemoteCall::Interrupt { reply, gate, .. }
            | RemoteCall::Answer { reply, gate, .. }
            | RemoteCall::Questions { reply, gate, .. }
            | RemoteCall::Shelve { reply, gate, .. } => reply.is_disconnected() || gate.cancelled(),
        }
    }

    pub fn refuse(self, error: RpcError) {
        // A send fails only if the client stopped waiting, and then there's nobody to tell.
        match self {
            RemoteCall::Start { reply, .. } => _ = reply.send(Err(error)),
            RemoteCall::Send { reply, .. }
            | RemoteCall::Interrupt { reply, .. }
            | RemoteCall::Answer { reply, .. }
            | RemoteCall::Questions { reply, .. }
            | RemoteCall::Shelve { reply, .. } => _ = reply.send(Err(error)),
        }
    }
}

/// The daemon's [`Agents`], answered by the Workspace on the UI thread.
pub struct UiAgents {
    calls: flume::Sender<RemoteCall>,
}

const REPLY_TIMEOUT: Duration = Duration::from_secs(30);
/// How often a waiting call checks whether its device was cut off, so its daemon thread is freed soon after.
const GATE_POLL: Duration = Duration::from_millis(50);

impl UiAgents {
    /// The receiver goes to `Workspace::set_remote`.
    pub fn new() -> (Self, flume::Receiver<RemoteCall>) {
        let (calls, rx) = flume::unbounded();
        (Self { calls }, rx)
    }

    fn ask<T>(&self, make: impl FnOnce(Reply<T>, CallGate) -> RemoteCall) -> Result<T, RpcError> {
        let gate = CallGate::current();
        let (reply, answer) = flume::bounded(1);
        if self.calls.send(make(reply, gate.clone())).is_err() {
            return Err(internal("the app is closing"));
        }
        // Dropping `answer` (returning) marks the call abandoned, so the app won't run it later.
        let deadline = std::time::Instant::now() + REPLY_TIMEOUT;
        loop {
            match answer.recv_timeout(GATE_POLL) {
                Ok(result) => return result,
                Err(flume::RecvTimeoutError::Disconnected) => {
                    return Err(internal("the app is closing"));
                }
                Err(flume::RecvTimeoutError::Timeout) if gate.cancelled() => {
                    return Err(RpcError::new(
                        ErrorCode::NotApproved,
                        "this device was cut off",
                    ));
                }
                Err(flume::RecvTimeoutError::Timeout) if std::time::Instant::now() >= deadline => {
                    return Err(internal("the app did not answer in time"));
                }
                Err(flume::RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

pub(crate) fn internal(message: &str) -> RpcError {
    RpcError::new(ErrorCode::Internal, message)
}

impl Agents for UiAgents {
    fn start_thread(&self, project_id: i64, prompt: &str) -> Result<i64, RpcError> {
        let prompt = prompt.to_owned();
        self.ask(|reply, gate| RemoteCall::Start {
            project_id,
            prompt,
            reply,
            gate,
        })
    }

    fn send_message(&self, thread_id: i64, text: &str) -> Result<(), RpcError> {
        let text = text.to_owned();
        self.ask(|reply, gate| RemoteCall::Send {
            thread_id,
            text,
            reply,
            gate,
        })
    }

    fn interrupt(&self, thread_id: i64) -> Result<(), RpcError> {
        self.ask(|reply, gate| RemoteCall::Interrupt {
            thread_id,
            reply,
            gate,
        })
    }

    fn answer_permission(
        &self,
        thread_id: i64,
        request_id: &str,
        decision: PermissionAnswer,
    ) -> Result<(), RpcError> {
        let request_id = request_id.to_owned();
        self.ask(|reply, gate| RemoteCall::Answer {
            thread_id,
            request_id,
            decision,
            reply,
            gate,
        })
    }

    fn answer_questions(
        &self,
        thread_id: i64,
        request_id: &str,
        answers: &[Vec<String>],
    ) -> Result<(), RpcError> {
        let (request_id, answers) = (request_id.to_owned(), answers.to_vec());
        self.ask(|reply, gate| RemoteCall::Questions {
            thread_id,
            request_id,
            answers,
            reply,
            gate,
        })
    }

    fn shelve_thread(
        &self,
        thread_id: i64,
        settled: bool,
        snoozed_until: Option<i64>,
    ) -> Result<(), RpcError> {
        self.ask(|reply, gate| RemoteCall::Shelve {
            thread_id,
            settled,
            snoozed_until,
            reply,
            gate,
        })
    }
}
