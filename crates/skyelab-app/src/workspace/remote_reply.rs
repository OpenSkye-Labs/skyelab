//! How the Workspace answers a remote client's call: in time, or with a refusal that names no local path.

use super::Workspace;
use crate::remote::Reply;
use gpui::Context;
use skyelab_protocol::{ErrorCode, RpcError};
use std::time::Duration;

/// How long a remote follow-up or interrupt waits to reach the agent; under the daemon's own 30 s wait, so the
/// client hears this answer rather than a timeout.
const DELIVERY_WAIT: Duration = Duration::from_secs(20);

/// Sends `delivered`'s result to `reply`, or a timeout error once [`DELIVERY_WAIT`] passes.
pub(super) fn answer_within(
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

pub(super) fn refused(message: impl Into<String>) -> RpcError {
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

pub(super) fn not_open(thread_id: i64) -> RpcError {
    RpcError::new(
        ErrorCode::NotFound,
        format!("thread {thread_id} isn't running here (it ended, or it was closed)"),
    )
}
