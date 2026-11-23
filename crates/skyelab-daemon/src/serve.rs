//! One admitted connection: requests, replies and pushed events until it closes.

use std::net::SocketAddr;
use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use skyelab_protocol::{
    Call, ErrorCode, Hello, Reply, Request, RpcError, ServerMessage, ThreadEvent, negotiate,
};
use tokio::sync::{Notify, Semaphore, broadcast, mpsc};
use tokio::task::JoinSet;
use tokio::time::{Instant, sleep_until, timeout};
use tokio_tungstenite::tungstenite::Message;

use crate::access::{self, Verdict, Who};
use crate::{Backend, Shared, Ws};

enum Step {
    Send(ServerMessage),
    SendAndClose(ServerMessage),
    Nothing,
    Close,
}

pub(crate) async fn serve(ws: Ws, peer: SocketAddr, shared: &Shared, who: &Who, kick: &Notify) {
    let config = &shared.config;
    let (mut sink, mut stream) = ws.split();
    let in_flight = Arc::new(Semaphore::new(config.max_in_flight));
    let (replies_tx, mut replies) = mpsc::channel(config.max_in_flight);
    let mut running = JoinSet::new();
    let mut events: Option<broadcast::Receiver<ThreadEvent>> = None;
    let mut idle_at = Instant::now() + config.idle_timeout;

    loop {
        let step = tokio::select! {
            // A revoke or Disconnect wins over a request that is already waiting to be read.
            biased;
            () = kick.notified() => {
                // No close handshake: a client that stopped reading would stall it, and dropping the socket closes it.
                eprintln!("skyelab-daemon: closing {peer}: disconnected by the user");
                break;
            }
            () = sleep_until(idle_at) => {
                eprintln!("skyelab-daemon: closing {peer}: idle");
                Step::Close
            }
            incoming = stream.next() => {
                idle_at = Instant::now() + config.idle_timeout;
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        on_text(&text, shared, who, &mut events, &in_flight, &replies_tx, &mut running)
                    }
                    Some(Ok(Message::Binary(_))) => Step::Send(ServerMessage::Error {
                        id: None,
                        error: RpcError::new(ErrorCode::BadRequest, "send JSON in text frames"),
                    }),
                    Some(Ok(Message::Close(_))) | None => Step::Close,
                    Some(Ok(_)) => Step::Nothing, // ping/pong; tungstenite answers pings itself
                    Some(Err(e)) => {
                        // Oversized messages and protocol violations land here.
                        eprintln!("skyelab-daemon: closing {peer}: {e}");
                        Step::Close
                    }
                }
            }
            Some(reply) = replies.recv() => Step::Send(reply),
            event = next_event(&mut events) => match event {
                Some(event) => Step::Send(ServerMessage::Event(event)),
                None => {
                    // seq is per thread, so a client can't see a gap on a thread whose events were all dropped.
                    eprintln!("skyelab-daemon: closing {peer}: too slow for pushed events, it must reconnect and resync");
                    Step::Close
                }
            },
            Some(_) = running.join_next(), if !running.is_empty() => Step::Nothing,
        };
        let (msg, close) = match step {
            Step::Send(m) => (Some(m), false),
            Step::SendAndClose(m) => (Some(m), true),
            Step::Nothing => (None, false),
            Step::Close => (None, true),
        };
        // A client that stops reading would block the send forever, and the idle timer can't fire meanwhile.
        // A revoke or Disconnect cuts a stalled write (or close) short: dropping the socket closes it at once.
        if let Some(msg) = msg {
            let msg = without_paths(msg);
            let sent = tokio::select! {
                biased;
                () = kick.notified() => {
                    eprintln!("skyelab-daemon: closing {peer}: disconnected by the user");
                    break;
                }
                sent = timeout(config.idle_timeout, sink.send(Message::text(msg.to_json()))) => sent,
            };
            if !matches!(sent, Ok(Ok(()))) {
                eprintln!("skyelab-daemon: closing {peer}: write failed or stalled");
                break;
            }
        }
        if close {
            tokio::select! {
                biased;
                () = kick.notified() => {}
                _ = timeout(config.idle_timeout, sink.close()) => {}
            }
            break;
        }
    }
    // Dropping `running` drops pending replies; a backend call already running still finishes on its thread.
}

fn on_text(
    text: &str,
    shared: &Shared,
    who: &Who,
    events: &mut Option<broadcast::Receiver<ThreadEvent>>,
    in_flight: &Arc<Semaphore>,
    replies: &mpsc::Sender<ServerMessage>,
    running: &mut JoinSet<()>,
) -> Step {
    let req = match Request::parse(text) {
        Ok(req) => req,
        Err(msg) => return Step::Send(msg),
    };
    let id = req.id;
    let error = |code, message: &str| ServerMessage::Error {
        id: Some(id),
        error: RpcError::new(code, message),
    };
    match (req.call, events.is_some()) {
        (
            Call::Hello {
                protocol,
                device_key,
                ..
            },
            false,
        ) => match access::check(&shared.access, &shared.peers, who, device_key.as_deref()) {
            Verdict::Approved => hello(id, protocol, shared, events),
            Verdict::NotApproved => {
                eprintln!("skyelab-daemon: {:?} isn't approved yet", who.device);
                Step::SendAndClose(error(
                    ErrorCode::NotApproved,
                    "this device isn't approved on this machine yet: allow it in SkyeLab's Remote tab there",
                ))
            }
            Verdict::Denied => Step::SendAndClose(error(
                ErrorCode::Denied,
                "this device was turned down on this machine",
            )),
        },
        (Call::Hello { .. }, true) => {
            Step::Send(error(ErrorCode::BadRequest, "already said hello"))
        }
        (_, false) => Step::Send(error(ErrorCode::HelloRequired, "send hello first")),
        (call, true) => {
            let Ok(permit) = in_flight.clone().try_acquire_owned() else {
                return Step::Send(error(
                    ErrorCode::TooManyRequests,
                    "too many requests in flight",
                ));
            };
            let backend = shared.backend.clone();
            let replies = replies.clone();
            let gate = who.gate.clone();
            running.spawn(async move {
                // Checked again here: the call can wait for a blocking thread while its device is revoked.
                let run = move || match gate.cancelled() {
                    true => Err(RpcError::new(
                        ErrorCode::NotApproved,
                        "this device was cut off",
                    )),
                    false => gate.run(|| dispatch(&*backend, call)),
                };
                let reply = match tokio::task::spawn_blocking(run).await {
                    Ok(Ok(result)) => ServerMessage::Response { id, result },
                    Ok(Err(error)) => ServerMessage::Error {
                        id: Some(id),
                        error,
                    },
                    Err(_) => ServerMessage::Error {
                        id: Some(id),
                        error: RpcError::new(ErrorCode::Internal, "request failed"),
                    },
                };
                // Waits only while `serve` is busy writing; it drains this channel every turn.
                let _ = replies.send(reply).await;
                drop(permit);
            });
            Step::Nothing
        }
    }
}

fn hello(
    id: skyelab_protocol::RequestId,
    protocol: skyelab_protocol::Version,
    shared: &Shared,
    events: &mut Option<broadcast::Receiver<ThreadEvent>>,
) -> Step {
    match negotiate(protocol) {
        Ok(protocol) => {
            *events = Some(shared.events.subscribe());
            Step::Send(ServerMessage::Response {
                id,
                result: Reply::Hello(Hello {
                    protocol,
                    capabilities: vec!["events".into()],
                    node_id: Some(shared.node_id.clone()),
                }),
            })
        }
        Err(error) => Step::SendAndClose(ServerMessage::Error {
            id: Some(id),
            error,
        }),
    }
}

fn dispatch(backend: &dyn Backend, call: Call) -> Result<Reply, RpcError> {
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
    }
}

/// Error text can name this machine's folders (a refusal that quotes one); a client never sees them.
fn without_paths(mut msg: ServerMessage) -> ServerMessage {
    if let ServerMessage::Error { error, .. } = &mut msg {
        error.message = redact_paths(&error.message);
    }
    msg
}

/// Every space-separated word with a `/` or `\` in it becomes `<local path>`.
// shortcut: a path with spaces keeps its slash-free middle words ("b" in "/a/b c/d"); fine while errors quote paths whole.
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

/// The next pushed event; `None` if this connection fell behind and missed some. Pends forever before hello.
async fn next_event(events: &mut Option<broadcast::Receiver<ThreadEvent>>) -> Option<ThreadEvent> {
    if let Some(rx) = events {
        match rx.recv().await {
            Ok(event) => return Some(event),
            Err(broadcast::error::RecvError::Lagged(_)) => return None,
            Err(broadcast::error::RecvError::Closed) => {}
        }
    }
    std::future::pending().await
}
