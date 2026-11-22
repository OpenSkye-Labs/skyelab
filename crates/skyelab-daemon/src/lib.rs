//! The environment daemon: a WebSocket RPC server that only this user's own tailnet devices can reach.
//!
//! Tokio lives here only, on the daemon's own thread. The app talks to it through [`Daemon`], which is plain
//! sync code, so GPUI never needs tokio.
//!
//! Security gate, in order, for every TCP connection: the listener is bound to this node's tailnet IPs only;
//! `whois(peer_addr)` must say it is one of the user's own untagged devices ([`is_own_device`]); the handshake must
//! not come from a browser (no `Origin`); the first request must be a compatible `hello`. Any error, including
//! Tailscale being down or slow, rejects the connection.

mod backend;

use std::net::{SocketAddr, TcpListener as StdListener};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures::{SinkExt, StreamExt};
use skyelab_protocol::{
    Call, ErrorCode, Hello, Reply, Request, RpcError, ServerMessage, ThreadEvent, negotiate,
};
use skyelab_tailnet::{Tailnet, is_own_device};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, broadcast, mpsc, oneshot};
use tokio::task::JoinSet;
use tokio::time::{Instant, sleep_until, timeout};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{
    ErrorResponse, Request as HttpRequest, Response,
};
use tokio_tungstenite::tungstenite::http::{StatusCode, header};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

pub use backend::{Agents, Backend, MAX_EVENTS_PER_REPLY, NotWired, StoreBackend, event};

pub type SharedTailnet = Arc<dyn Tailnet + Send + Sync>;

#[derive(Debug, Clone)]
pub struct Config {
    /// The same port on every tailnet IP. 0 picks a free one per IP (tests).
    pub port: u16,
    /// Largest message or frame; a bigger one closes the connection.
    pub max_message_bytes: usize,
    /// Requests running at once per connection; more get `too_many_requests`.
    pub max_in_flight: usize,
    /// A connection that sends nothing for this long is closed.
    pub idle_timeout: Duration,
    /// Bound on whois plus the WebSocket handshake. Past it the connection is rejected.
    pub gate_timeout: Duration,
    /// Connections being gated at once; more are dropped, so a flood can't spawn unbounded whois calls.
    pub max_pending: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 7287,
            max_message_bytes: 1 << 20,
            max_in_flight: 16,
            idle_timeout: Duration::from_secs(300),
            gate_timeout: Duration::from_secs(10),
            max_pending: 16,
        }
    }
}

/// The running server. Dropping it stops the server and closes every connection.
pub struct Daemon {
    addrs: Vec<SocketAddr>,
    events: broadcast::Sender<ThreadEvent>,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

struct Shared {
    config: Config,
    tailnet: SharedTailnet,
    backend: Arc<dyn Backend>,
    events: broadcast::Sender<ThreadEvent>,
    pending: Arc<Semaphore>,
}

impl Daemon {
    /// Binds to this node's tailnet IPs and serves on a new thread. Fails if Tailscale isn't up.
    pub fn start(
        config: Config,
        tailnet: SharedTailnet,
        backend: Arc<dyn Backend>,
    ) -> Result<Self> {
        if config.max_in_flight == 0 || config.max_pending == 0 || config.max_message_bytes == 0 {
            bail!("daemon limits must be at least 1: {config:?}");
        }
        let me = tailnet
            .self_node()
            .context("can't start the daemon without a running tailnet")?;
        if me.tailscale_ips.is_empty() {
            bail!("this node has no tailnet IP to listen on");
        }
        let mut listeners = Vec::new();
        for ip in &me.tailscale_ips {
            // Never a wildcard, including ::ffff:0.0.0.0, which dual-stack Linux binds as IPv4 0.0.0.0.
            if ip.to_canonical().is_unspecified() {
                bail!("tailnet reported an unspecified address ({ip}); refusing to listen on it");
            }
            let addr = SocketAddr::new(*ip, config.port);
            let l = StdListener::bind(addr).with_context(|| format!("listen on {addr}"))?;
            l.set_nonblocking(true)
                .context("make listener non-blocking")?;
            listeners.push(l);
        }
        let addrs = listeners
            .iter()
            .map(|l| l.local_addr())
            .collect::<std::io::Result<_>>()
            .context("read listener address")?;

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("skyelab-daemon")
            .enable_all()
            .build()
            .context("start the daemon runtime")?;
        let listeners = {
            let _runtime = runtime.enter();
            listeners
                .into_iter()
                .map(TcpListener::from_std)
                .collect::<std::io::Result<Vec<_>>>()
                .context("register listener")?
        };
        let (events, _) = broadcast::channel(256);
        let shared = Arc::new(Shared {
            pending: Arc::new(Semaphore::new(config.max_pending)),
            config,
            tailnet,
            backend,
            events: events.clone(),
        });
        let (shutdown, stop) = oneshot::channel();
        let thread = std::thread::Builder::new()
            .name("skyelab-daemon".into())
            .spawn(move || {
                runtime.block_on(async move {
                    for l in listeners {
                        tokio::spawn(accept_loop(l, shared.clone()));
                    }
                    let _ = stop.await;
                });
                // Blocking whois calls can't be cancelled; their CLI timeout bounds them.
                runtime.shutdown_timeout(Duration::from_secs(1));
            })
            .context("spawn the daemon thread")?;
        Ok(Self {
            addrs,
            events,
            shutdown: Some(shutdown),
            thread: Some(thread),
        })
    }

    pub fn local_addrs(&self) -> &[SocketAddr] {
        &self.addrs
    }

    /// Pushes `event` to every connection that has said hello. Never blocks.
    pub fn push_event(&self, event: ThreadEvent) {
        // No receivers just means nobody is connected.
        let _ = self.events.send(event);
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

async fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                // EMFILE and friends: back off instead of spinning.
                eprintln!("skyelab-daemon: accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let Ok(permit) = shared.pending.clone().try_acquire_owned() else {
            eprintln!("skyelab-daemon: rejected {peer}: too many connections waiting for whois");
            continue;
        };
        tokio::spawn(handle(stream, peer, shared.clone(), permit));
    }
}

async fn handle(
    stream: TcpStream,
    peer: SocketAddr,
    shared: Arc<Shared>,
    permit: OwnedSemaphorePermit,
) {
    let ws = match timeout(
        shared.config.gate_timeout,
        admit(stream, peer, &shared, permit),
    )
    .await
    {
        Ok(Ok(ws)) => ws,
        Ok(Err(reason)) => {
            eprintln!("skyelab-daemon: rejected {peer}: {reason}");
            return;
        }
        Err(_) => {
            eprintln!(
                "skyelab-daemon: rejected {peer}: whois or handshake took over {:?}",
                shared.config.gate_timeout
            );
            return;
        }
    };
    serve(ws, peer, &shared).await;
}

/// The gate. Nothing is read from the socket until whois has passed.
async fn admit(
    stream: TcpStream,
    peer: SocketAddr,
    shared: &Shared,
    permit: OwnedSemaphorePermit,
) -> Result<WebSocketStream<TcpStream>, String> {
    let tailnet = shared.tailnet.clone();
    // The permit lives in the blocking task: a gate timeout can't cancel whois, so the slot frees when whois ends.
    let checked = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let me = tailnet
            .self_node()
            .map_err(|e| format!("tailnet unavailable: {e}"))?;
        let who = tailnet
            .whois(peer)
            .map_err(|e| format!("whois failed: {e}"))?;
        if is_own_device(&me, &who) {
            Ok(who)
        } else {
            Err(format!(
                "not one of your devices: node {:?} ({}), user {:?}, tags {:?}",
                who.node.name, who.node.stable_id, who.user_profile.login_name, who.node.tags
            ))
        }
    })
    .await
    .map_err(|e| format!("whois task failed: {e}"))??;

    let limit = shared.config.max_message_bytes;
    let ws_config = WebSocketConfig::default()
        .max_message_size(Some(limit))
        .max_frame_size(Some(limit));
    let ws =
        tokio_tungstenite::accept_hdr_async_with_config(stream, reject_browsers, Some(ws_config))
            .await
            .map_err(|e| format!("handshake from {:?} failed: {e}", checked.node.name))?;
    eprintln!(
        "skyelab-daemon: accepted {peer}: {:?} ({})",
        checked.node.name, checked.user_profile.login_name
    );
    Ok(ws)
}

/// A web page open on one of the user's devices passes whois too. Browsers always send `Origin`; native clients don't.
#[allow(clippy::result_large_err)] // the signature tungstenite's callback requires
fn reject_browsers(req: &HttpRequest, resp: Response) -> Result<Response, ErrorResponse> {
    if req.headers().contains_key(header::ORIGIN) {
        let mut err = ErrorResponse::new(Some("browser connections are not allowed".into()));
        *err.status_mut() = StatusCode::FORBIDDEN;
        return Err(err);
    }
    Ok(resp)
}

enum Step {
    Send(ServerMessage),
    SendAndClose(ServerMessage),
    Nothing,
    Close,
}

async fn serve(ws: WebSocketStream<TcpStream>, peer: SocketAddr, shared: &Shared) {
    let config = &shared.config;
    let (mut sink, mut stream) = ws.split();
    let in_flight = Arc::new(Semaphore::new(config.max_in_flight));
    let (replies_tx, mut replies) = mpsc::channel(config.max_in_flight);
    let mut running = JoinSet::new();
    let mut events: Option<broadcast::Receiver<ThreadEvent>> = None;
    let mut idle_at = Instant::now() + config.idle_timeout;

    loop {
        let step = tokio::select! {
            () = sleep_until(idle_at) => {
                eprintln!("skyelab-daemon: closing {peer}: idle");
                Step::Close
            }
            incoming = stream.next() => {
                idle_at = Instant::now() + config.idle_timeout;
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        on_text(&text, shared, &mut events, &in_flight, &replies_tx, &mut running)
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
        if let Some(msg) = msg {
            let sent = timeout(config.idle_timeout, sink.send(Message::text(msg.to_json()))).await;
            if !matches!(sent, Ok(Ok(()))) {
                eprintln!("skyelab-daemon: closing {peer}: write failed or stalled");
                break;
            }
        }
        if close {
            let _ = timeout(config.idle_timeout, sink.close()).await;
            break;
        }
    }
    // Dropping `running` drops pending replies; a backend call already running still finishes on its thread.
}

fn on_text(
    text: &str,
    shared: &Shared,
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
        (Call::Hello { protocol, .. }, false) => match negotiate(protocol) {
            Ok(protocol) => {
                *events = Some(shared.events.subscribe());
                Step::Send(ServerMessage::Response {
                    id,
                    result: Reply::Hello(Hello {
                        protocol,
                        capabilities: vec!["events".into()],
                    }),
                })
            }
            Err(error) => Step::SendAndClose(ServerMessage::Error {
                id: Some(id),
                error,
            }),
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
            running.spawn(async move {
                let reply =
                    match tokio::task::spawn_blocking(move || dispatch(&*backend, call)).await {
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
