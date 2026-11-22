//! The daemon's agent calls, forwarded to the UI thread so they run through the thread view's own launch path.

use skyelab_daemon::Agents;
use skyelab_protocol::{ErrorCode, RpcError};
use std::time::Duration;

pub type Reply<T> = flume::Sender<Result<T, RpcError>>;

pub enum RemoteCall {
    Start {
        project_id: i64,
        prompt: String,
        reply: Reply<i64>,
    },
    Send {
        thread_id: i64,
        text: String,
        reply: Reply<()>,
    },
    Interrupt {
        thread_id: i64,
        reply: Reply<()>,
    },
}

impl RemoteCall {
    /// Its client stopped waiting (timed out, or the daemon went away). Running it now would surprise them.
    pub fn abandoned(&self) -> bool {
        match self {
            RemoteCall::Start { reply, .. } => reply.is_disconnected(),
            RemoteCall::Send { reply, .. } | RemoteCall::Interrupt { reply, .. } => {
                reply.is_disconnected()
            }
        }
    }

    pub fn refuse(self, error: RpcError) {
        // A send fails only if the client stopped waiting, and then there's nobody to tell.
        match self {
            RemoteCall::Start { reply, .. } => _ = reply.send(Err(error)),
            RemoteCall::Send { reply, .. } | RemoteCall::Interrupt { reply, .. } => {
                _ = reply.send(Err(error))
            }
        }
    }
}

/// The daemon's [`Agents`], answered by the Workspace on the UI thread.
pub struct UiAgents {
    calls: flume::Sender<RemoteCall>,
}

const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

impl UiAgents {
    /// The receiver goes to `Workspace::set_remote`.
    pub fn new() -> (Self, flume::Receiver<RemoteCall>) {
        let (calls, rx) = flume::unbounded();
        (Self { calls }, rx)
    }

    fn ask<T>(&self, make: impl FnOnce(Reply<T>) -> RemoteCall) -> Result<T, RpcError> {
        let (reply, answer) = flume::bounded(1);
        if self.calls.send(make(reply)).is_err() {
            return Err(internal("the app is closing"));
        }
        answer.recv_timeout(REPLY_TIMEOUT).map_err(|e| match e {
            // Dropping `answer` marks the call abandoned, so the app won't run it later.
            flume::RecvTimeoutError::Timeout => internal("the app did not answer in time"),
            flume::RecvTimeoutError::Disconnected => internal("the app is closing"),
        })?
    }
}

pub(crate) fn internal(message: &str) -> RpcError {
    RpcError::new(ErrorCode::Internal, message)
}

impl Agents for UiAgents {
    fn start_thread(&self, project_id: i64, prompt: &str) -> Result<i64, RpcError> {
        let prompt = prompt.to_owned();
        self.ask(|reply| RemoteCall::Start {
            project_id,
            prompt,
            reply,
        })
    }

    fn send_message(&self, thread_id: i64, text: &str) -> Result<(), RpcError> {
        let text = text.to_owned();
        self.ask(|reply| RemoteCall::Send {
            thread_id,
            text,
            reply,
        })
    }

    fn interrupt(&self, thread_id: i64) -> Result<(), RpcError> {
        self.ask(|reply| RemoteCall::Interrupt { thread_id, reply })
    }
}
