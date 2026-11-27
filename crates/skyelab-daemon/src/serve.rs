//! One admitted connection: requests, replies and pushed events until it closes.

use std::net::SocketAddr;
use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use skyelab_protocol::{
    Call, ErrorCode, Hello, Reply, Request, RpcError, ServerMessage, ShellChange,
    TUNNEL_CAPABILITY, ThreadEvent, negotiate,
};
use tokio::sync::{Notify, Semaphore, broadcast, mpsc};
use tokio::task::JoinSet;
use tokio::time::{Instant, sleep_until, timeout};
use tokio_tungstenite::tungstenite::Message;

use crate::access::{self, Verdict, Who};
use crate::peers::lock_peers;
use crate::{Shared, Ws, forward};

#[path = "serve_dispatch.rs"]
mod dispatch;
use dispatch::{dispatch, without_paths};

/// A reply, and for `subscribe_shell` its arrival number and the changes to forward once it is written.
type Out = (ServerMessage, Option<(u64, flume::Receiver<ShellChange>)>);

/// Where replies go, and how many `subscribe_shell` calls this connection sent: each is numbered as it arrives, so
/// the newest one's stream wins however the replies cross (request ids needn't increase).
struct Outbox {
    tx: mpsc::Sender<Out>,
    subscribes: u64,
}

enum Step {
    Send(ServerMessage),
    /// `open_tunnel` was admitted: dial the port, answer, then carry bytes until it ends.
    Tunnel {
        id: skyelab_protocol::RequestId,
        port: u16,
        ended: tokio::sync::watch::Receiver<()>,
    },
    SendAndClose(ServerMessage),
    Nothing,
    Close,
}

pub(crate) async fn serve(ws: Ws, peer: SocketAddr, shared: &Shared, who: &Who, kick: &Notify) {
    let config = &shared.config;
    let (mut sink, mut stream) = ws.split();
    let in_flight = Arc::new(Semaphore::new(config.max_in_flight));
    let (tx, mut replies) = mpsc::channel(config.max_in_flight);
    let mut outbox = Outbox { tx, subscribes: 0 };
    let mut running = JoinSet::new();
    let mut events: Option<broadcast::Receiver<ThreadEvent>> = None;
    // Set with the snapshot reply, so no change goes out before the snapshot it follows. Kept with its arrival
    // number: subscribes are answered as they finish, and an older one's late reply must not replace a newer stream.
    let mut shell: Option<(u64, flume::Receiver<ShellChange>)> = None;
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
                        on_text(&text, shared, who, &mut events, &in_flight, &mut outbox, &mut running)
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
            Some((reply, changes)) = replies.recv() => {
                if let Some((arrival, changes)) = changes
                    && shell.as_ref().is_none_or(|(newest, _)| arrival > *newest)
                {
                    shell = Some((arrival, changes));
                }
                Step::Send(reply)
            }
            change = next_change(&mut shell) => match change {
                Some(change) => Step::Send(ServerMessage::Shell(change)),
                None => {
                    eprintln!("skyelab-daemon: closing {peer}: too slow for the shell stream, it must reconnect and resubscribe");
                    Step::Close
                }
            },
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
            Step::Tunnel { id, port, ended } => {
                let tcp = tokio::select! {
                    biased;
                    () = kick.notified() => break,
                    tcp = forward::dial_loopback(port) => tcp,
                };
                let reply = match &tcp {
                    Ok(_) => ServerMessage::Response {
                        id,
                        result: Reply::Ok,
                    },
                    Err(why) => ServerMessage::Error {
                        id: Some(id),
                        error: RpcError::new(ErrorCode::NotFound, why),
                    },
                };
                let sent = tokio::select! {
                    biased;
                    () = kick.notified() => break,
                    sent = timeout(config.idle_timeout, sink.send(Message::text(reply.to_json()))) => sent,
                };
                if let (Ok(tcp), Ok(Ok(()))) = (tcp, sent) {
                    eprintln!("skyelab-daemon: {peer} tunnels to loopback port {port}");
                    forward::pump(sink, stream, tcp, ended, kick, config.idle_timeout).await;
                }
                break;
            }
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
    // Dropping `running` drops pending replies; a running backend call still finishes on its thread.
}

fn on_text(
    text: &str,
    shared: &Shared,
    who: &Who,
    events: &mut Option<broadcast::Receiver<ThreadEvent>>,
    in_flight: &Arc<Semaphore>,
    replies: &mut Outbox,
    running: &mut JoinSet<()>,
) -> Step {
    let mut timed = skyelab_core::debug::RpcCall::start("daemon", text);
    let step = (|| {
        let req = match Request::parse(text) {
            Ok(req) => req,
            Err(msg) => return Step::Send(msg),
        };
        let id = req.id;
        let error = move |code, message: &str| ServerMessage::Error {
            id: Some(id),
            error: RpcError::new(code, message),
        };
        // Set at hello; a tunnel connection may only open its tunnel.
        let tunnel = lock_peers(&shared.peers)
            .get(&who.peer_id)
            .is_some_and(|e| e.tunnel);
        match (req.call, events.is_some()) {
            (
                Call::Hello {
                    protocol,
                    device_key,
                    capabilities,
                },
                false,
            ) => {
                // Marked before the approval lands on the entry, so a tunnel is never listed as a device.
                let tunnel = capabilities.iter().any(|c| c == TUNNEL_CAPABILITY);
                if tunnel && let Some(entry) = lock_peers(&shared.peers).get_mut(&who.peer_id) {
                    entry.tunnel = true;
                }
                hello_checked(
                    id,
                    protocol,
                    tunnel,
                    device_key.as_deref(),
                    shared,
                    who,
                    events,
                )
            }
            (Call::Hello { .. }, true) => {
                Step::Send(error(ErrorCode::BadRequest, "already said hello"))
            }
            (_, false) => Step::Send(error(ErrorCode::HelloRequired, "send hello first")),
            (Call::OpenTunnel { port, token }, true) if tunnel => {
                match forward::join(&shared.peers, who.peer_id, port, &token) {
                    Ok(ended) => Step::Tunnel { id, port, ended },
                    Err(why) => Step::SendAndClose(error(ErrorCode::BadRequest, &why)),
                }
            }
            // Hidden from the device list and Disconnect's reach, so it may do nothing else.
            (_, true) if tunnel => Step::SendAndClose(error(
                ErrorCode::BadRequest,
                "a tunnel connection can only open a tunnel",
            )),
            (Call::OpenTunnel { .. }, true) => Step::Send(error(
                ErrorCode::BadRequest,
                "open_tunnel needs a connection that said hello as a tunnel",
            )),
            (call, true) => {
                let incoming = Incoming {
                    id,
                    call,
                    timed: timed.take(),
                };
                on_call(incoming, shared, who, in_flight, replies, running)
            }
        }
    })();
    log_reply(timed, &step);
    step
}

/// Logs a request answered here and now (the backend and forward tasks log their own).
fn log_reply(timed: Option<skyelab_core::debug::RpcCall>, step: &Step) {
    match step {
        Step::Send(msg) | Step::SendAndClose(msg) => log_message(timed, msg),
        _ => {
            if let Some(timed) = timed {
                timed.done(None);
            }
        }
    }
}

/// Logs the reply to a timed request: its error, or success.
fn log_message(timed: Option<skyelab_core::debug::RpcCall>, msg: &ServerMessage) {
    if let Some(timed) = timed {
        match msg {
            ServerMessage::Error { error, .. } => timed.done(Some(&error.message)),
            _ => timed.done(None),
        }
    }
}

/// Hello's device-key check, then the version.
fn hello_checked(
    id: skyelab_protocol::RequestId,
    protocol: skyelab_protocol::Version,
    tunnel: bool,
    device_key: Option<&str>,
    shared: &Shared,
    who: &Who,
    events: &mut Option<broadcast::Receiver<ThreadEvent>>,
) -> Step {
    let error = move |code, message: &str| ServerMessage::Error {
        id: Some(id),
        error: RpcError::new(code, message),
    };
    match access::check(&shared.access, &shared.peers, who, device_key) {
        Verdict::Approved => hello(id, protocol, tunnel, shared, events),
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
    }
}

/// A request past hello, and its timing for the debug log (`None` while debug mode is off).
struct Incoming {
    id: skyelab_protocol::RequestId,
    call: Call,
    timed: Option<skyelab_core::debug::RpcCall>,
}

/// A call after hello on a connection that isn't a tunnel. Backend calls are timed for the debug log.
fn on_call(
    Incoming {
        id,
        call,
        mut timed,
    }: Incoming,
    shared: &Shared,
    who: &Who,
    in_flight: &Arc<Semaphore>,
    replies: &mut Outbox,
    running: &mut JoinSet<()>,
) -> Step {
    let error = move |code, message: &str| ServerMessage::Error {
        id: Some(id),
        error: RpcError::new(code, message),
    };
    let step = (|| {
        match call {
            Call::ForwardPort { port } => {
                if let Some(why) = forward::refusal(port, &shared.own_ports) {
                    return Step::Send(error(ErrorCode::BadRequest, &why));
                }
                let Ok(permit) = in_flight.clone().try_acquire_owned() else {
                    return Step::Send(error(
                        ErrorCode::TooManyRequests,
                        "too many requests in flight",
                    ));
                };
                let (peers, replies, peer) =
                    (shared.peers.clone(), replies.tx.clone(), who.peer_id);
                let timed = timed.take();
                running.spawn(async move {
                    // Probe first, so the user hears at once that nothing listens there.
                    let reply = match forward::dial_loopback(port).await {
                        Err(why) => error(ErrorCode::NotFound, &why),
                        Ok(_probe) => match forward::allow(&peers, peer, port) {
                            Ok(token) => ServerMessage::Response {
                                id,
                                result: Reply::Forwarded { token },
                            },
                            Err(why) => error(ErrorCode::BadRequest, &why),
                        },
                    };
                    log_message(timed, &reply);
                    let _ = replies.send((reply, None)).await;
                    drop(permit);
                });
                Step::Nothing
            }
            Call::StopForward { port, token } => {
                forward::stop(&shared.peers, who.peer_id, port, &token);
                Step::Send(ServerMessage::Response {
                    id,
                    result: Reply::Ok,
                })
            }
            call => {
                let subscribe = matches!(call, Call::SubscribeShell);
                let Ok(permit) = in_flight.clone().try_acquire_owned() else {
                    return Step::Send(error(
                        ErrorCode::TooManyRequests,
                        "too many requests in flight",
                    ));
                };
                let arrival = subscribe.then(|| {
                    replies.subscribes += 1;
                    replies.subscribes
                });
                let backend = shared.backend.clone();
                let replies = replies.tx.clone();
                let gate = who.gate.clone();
                let timed = timed.take();
                let ctx = crate::serve_delegate::Ctx::new(shared, who);
                running.spawn(async move {
                    // The call can wait for a blocking thread while its device is revoked.
                    let run = move || match gate.cancelled() {
                        true => Err(RpcError::new(
                            ErrorCode::NotApproved,
                            "this device was cut off",
                        )),
                        false if subscribe => gate.run(|| {
                            backend.subscribe_shell().map(|f| {
                                (
                                    Reply::Shell(crate::backend::bound(f.snapshot)),
                                    Some(f.changes),
                                )
                            })
                        }),
                        false => gate.run(|| dispatch(&*backend, call, &ctx).map(|r| (r, None))),
                    };
                    let (reply, changes) = match tokio::task::spawn_blocking(run).await {
                        Ok(Ok((result, changes))) => {
                            (ServerMessage::Response { id, result }, changes)
                        }
                        Ok(Err(error)) => (
                            ServerMessage::Error {
                                id: Some(id),
                                error,
                            },
                            None,
                        ),
                        Err(_) => (
                            ServerMessage::Error {
                                id: Some(id),
                                error: RpcError::new(ErrorCode::Internal, "request failed"),
                            },
                            None,
                        ),
                    };
                    log_message(timed, &reply);
                    // Waits only while `serve` is busy writing; it drains this channel every turn.
                    let changes = arrival.zip(changes);
                    let _ = replies.send((reply, changes)).await;
                    drop(permit);
                });
                Step::Nothing
            }
        }
    })();
    log_reply(timed, &step);
    step
}

fn hello(
    id: skyelab_protocol::RequestId,
    protocol: skyelab_protocol::Version,
    tunnel: bool,
    shared: &Shared,
    events: &mut Option<broadcast::Receiver<ThreadEvent>>,
) -> Step {
    match negotiate(protocol) {
        Ok(protocol) => {
            // A tunnel gets no pushed events: its next frame must be the `open_tunnel` reply. A receiver whose
            // sender is gone never yields one.
            *events = Some(match tunnel {
                true => broadcast::channel(1).1,
                false => shared.events.subscribe(),
            });
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

/// The next shell change; `None` once the stream ended (this connection fell behind). Pends forever before
/// `subscribe_shell` was answered.
async fn next_change(
    shell: &mut Option<(u64, flume::Receiver<ShellChange>)>,
) -> Option<ShellChange> {
    match shell {
        Some((_, rx)) => rx.recv_async().await.ok(),
        None => std::future::pending().await,
    }
}
