//! A backend call's reply, and what error text may leave this machine.

use skyelab_protocol::{Call, ErrorCode, Reply, RpcError, ServerMessage};

use crate::Backend;

#[cfg_attr(feature = "hotpath", hotpath::measure)]
#[tracing::instrument(name = "daemon.dispatch", skip_all)]
pub(super) fn dispatch(
    backend: &dyn Backend,
    call: Call,
    ctx: &crate::serve_delegate::Ctx,
) -> Result<Reply, RpcError> {
    match call {
        Call::Hello { .. } => Err(RpcError::new(ErrorCode::BadRequest, "already said hello")),
        Call::ListProjects => backend.list_projects().map(Reply::Projects),
        Call::ListThreads { project_id } => backend.list_threads(project_id).map(Reply::Threads),
        Call::ThreadEventsSince { thread_id, seq } => backend
            .thread_events_since(thread_id, seq)
            .map(Reply::Events),
        Call::StartThread { project_id, prompt } => backend
            .start_thread(project_id, &prompt)
            .map(|thread_id| Reply::ThreadStarted { thread_id }),
        Call::SendMessage { thread_id, text } => {
            backend.send_message(thread_id, &text).map(|()| Reply::Ok)
        }
        Call::Interrupt { thread_id } => backend.interrupt(thread_id).map(|()| Reply::Ok),
        // `on_text` serves these itself: they act on the connection, not the backend.
        Call::ForwardPort { .. }
        | Call::StopForward { .. }
        | Call::OpenTunnel { .. }
        | Call::SubscribeShell => Err(RpcError::new(ErrorCode::BadRequest, "not a backend call")),
        Call::AnswerPermission {
            thread_id,
            request_id,
            decision,
        } => backend
            .answer_permission(thread_id, &request_id, decision)
            .map(|()| Reply::Ok),
        Call::AnswerQuestions {
            thread_id,
            request_id,
            answers,
        } => backend
            .answer_questions(thread_id, &request_id, &answers)
            .map(|()| Reply::Ok),
        Call::Delegate { caller, op } => ctx.run(backend, caller, op).map(Reply::Delegated),
        Call::ShelveThread {
            thread_id,
            settled,
            snoozed_until,
        } => backend
            .shelve_thread(thread_id, settled, snoozed_until)
            .map(|()| Reply::Ok),
    }
}

/// Error text can name this machine's folders (a refusal that quotes one). The app's refusals are written
/// without paths; this catches the rest.
pub(super) fn without_paths(mut msg: ServerMessage) -> ServerMessage {
    if let ServerMessage::Error { error, .. } = &mut msg {
        error.message = redact_paths(&error.message);
    }
    msg
}

/// Every space-separated word with a `/` or `\` in it becomes `<local path>`.
// shortcut: a path with spaces keeps its slash-free words ("North" in "/work/Project North"); errors that may
// quote a path must be written without it at the source (workspace/remote.rs), as the known ones are.
fn redact_paths(message: &str) -> String {
    message
        .split(' ')
        .map(|word| match word.contains(['/', '\\']) {
            true => "<local path>",
            false => word,
        })
        .collect::<Vec<_>>()
        .join(" ")
}
