//! The daemon's agent calls, forwarded to the UI thread so they run through the thread view's own launch path.

use skyelab_daemon::Agents;
use skyelab_protocol::{ErrorCode, RpcError};
use std::time::Duration;

pub type Reply<T> = flume::Sender<Result<T, RpcError>>;

/// One agent call from a remote client, for the Workspace to run. Each carries where to send its answer.
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

/// Implements the daemon's [`Agents`] by asking the Workspace. Called on a daemon blocking thread, so it may block.
pub struct UiAgents {
    calls: flume::Sender<RemoteCall>,
}

/// Long enough for a store write and `claude` to spawn; a UI that hangs past it gets an error, not a stuck client.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

impl UiAgents {
    /// The receiver goes to `Workspace::serve_remote_calls`.
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
            flume::RecvTimeoutError::Timeout => internal("the app did not answer in time"),
            // The Workspace dropped the call without answering (its window closed).
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
