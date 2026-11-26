//! The tokio side: one task owns the connection, the request table and every watched thread's seq.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use rustls::ClientConfig;
use skyelab_protocol::{Call, Hello, Reply, Request, RequestId, ServerMessage};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::{Instant, sleep_until, timeout};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;

use crate::{Disconnect, DisconnectKind, Error, Options, Update, WatchId};

mod connect;
mod ctx;
mod expire;
pub(crate) mod forward;
mod forward_calls;
mod watched;
use connect::connect;
use forward::{Forward, Forwarding};
use watched::Watched;

/// The longest wait between tries while the machine hasn't approved this device yet.
const NOT_APPROVED_RETRY: Duration = Duration::from_secs(3);

pub(crate) type ReplyFn = Box<dyn FnOnce(Result<Reply, Error>) + Send>;

pub(crate) enum Command {
    Call {
        call: Call,
        reply: ReplyFn,
    },
    Watch {
        id: WatchId,
        after_seq: i64,
    },
    Unwatch(WatchId),
    Approve(Hello),
    Forward {
        port: u16,
        reply: flume::Sender<Result<Forward, Error>>,
    },
}

enum End {
    /// The client was dropped.
    Stopped,
    Fatal(Error),
    /// Retry. `was_up`: the connection got past hello, so the backoff starts over.
    Lost {
        why: Disconnect,
        was_up: bool,
    },
}

fn lost_as(kind: DisconnectKind, message: impl Into<String>) -> End {
    End::Lost {
        why: Disconnect {
            kind,
            message: message.into(),
        },
        was_up: false,
    }
}

fn lost(message: impl Into<String>) -> End {
    lost_as(DisconnectKind::Other, message)
}

struct Ctx {
    host: String,
    options: Options,
    /// `options.device_key`, loaded.
    device_key: Option<String>,
    tls: Arc<ClientConfig>,
    commands: flume::Receiver<Command>,
    commands_tx: flume::WeakSender<Command>,
    updates: flume::Sender<Update>,
    hello: Arc<Mutex<Option<Hello>>>,
    watched: HashMap<i64, Watched>,
    forwarding: Forwarding,
    /// Tunnels being dialled, across every forward (the daemon's gate drops a 5th pending one per address).
    dials: Arc<tokio::sync::Semaphore>,
}

pub(crate) async fn run(
    host: String,
    options: Options,
    tls: Arc<ClientConfig>,
    commands: flume::Receiver<Command>,
    commands_tx: flume::WeakSender<Command>,
    updates: flume::Sender<Update>,
    hello: Arc<Mutex<Option<Hello>>>,
) {
    // Debug mode's live count (docs/debugging.md): one per machine this app is connected to or retrying.
    let _live = skyelab_core::debug::Live::new("remote connections");
    // Before anything else runs on this client's own runtime, so blocking here (a keychain prompt) stalls only it.
    let device_key = match options.device_key.as_ref().map(crate::LazyDeviceKey::get) {
        None => None,
        Some(Ok(key)) => Some(key),
        Some(Err(e)) => {
            let _ = updates.send(Update::Fatal(crate::Error::DeviceKey(e)));
            return;
        }
    };
    let mut cx = Ctx {
        host,
        options,
        device_key,
        tls,
        commands,
        commands_tx,
        updates,
        hello,
        watched: HashMap::new(),
        forwarding: Forwarding::default(),
        dials: Arc::new(tokio::sync::Semaphore::new(forward::MAX_DIALS)),
    };
    let mut backoff = cx.options.backoff_min;
    loop {
        let (why, was_up) = match session(&mut cx).await {
            End::Stopped => return,
            End::Fatal(e) => {
                let _ = cx.updates.send(Update::Fatal(e));
                return;
            }
            End::Lost { why, was_up } => (why, was_up),
        };
        cx.set_hello(None);
        // Waiting for the user to click Allow there: try again soon, so it connects within seconds of that.
        let waiting = why.kind == DisconnectKind::NotApproved;
        let _ = cx.updates.send(Update::Disconnected(why));
        if was_up {
            backoff = cx.options.backoff_min;
        }
        if waiting {
            backoff = backoff.min(NOT_APPROVED_RETRY);
        }
        let retry_at = Instant::now() + backoff;
        loop {
            tokio::select! {
                () = sleep_until(retry_at) => break,
                command = cx.commands.recv_async() => if !cx.offline(command) { return },
            }
        }
        backoff = backoff.saturating_mul(2).min(cx.options.backoff_max);
    }
}

/// TLS in every shipped build; a bare TCP stream only in the test-only plaintext mode.
trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

type Ws = WebSocketStream<Box<dyn Io>>;

async fn session(cx: &mut Ctx) -> End {
    let connecting = timeout(cx.options.connect_timeout, connect(cx.target()));
    tokio::pin!(connecting);
    let (ws, hello) = loop {
        tokio::select! {
            result = &mut connecting => match result {
                Ok(Ok(up)) => break up,
                Ok(Err(end)) => return end,
                Err(_) => return lost(format!("connecting took over {:?}", cx.options.connect_timeout)),
            },
            command = cx.commands.recv_async() => if !cx.offline(command) { return End::Stopped },
        }
    };
    cx.set_hello(Some(hello.clone()));
    let _ = cx.updates.send(Update::Connected(hello.clone()));
    let mut live = Live {
        held: cx.options.approve_each_connection.then_some(hello),
        ws,
        next_id: 1,
        pending: HashMap::new(),
        timing: HashMap::new(),
        retry_at: None,
        retry_delay: cx.options.backoff_min,
        session_ended: tokio::sync::watch::channel(()).0,
    };
    let end = live.serve(cx).await;
    // Whatever was in flight on this connection can't be answered now.
    for (_, call) in live.timing.drain() {
        call.done(Some("connection lost"));
    }
    for (_, (_, waiter)) in live.pending.drain() {
        match waiter {
            Waiter::User(reply) => reply(Err(Error::ConnectionLost)),
            Waiter::Forward(_, _, reply) => {
                let _ = reply.send(Err(Error::ConnectionLost));
            }
            Waiter::Fill(_) => {}
        }
    }
    // Every forward of this connection has stopped with it (`session_ended`).
    cx.forwarding.clear();
    let _ = timeout(Duration::from_secs(1), live.ws.close(None)).await;
    match end {
        End::Lost { why, .. } => End::Lost { why, was_up: true },
        end => end,
    }
}

enum Waiter {
    User(ReplyFn),
    Fill(i64),
    /// `forward_port` for this port, under this reservation. Never expires: a reply after the caller gave up would
    /// leave the environment forwarding a port no `Forward` stops. The daemon's probe bounds it (10 s).
    Forward(u16, u64, flume::Sender<Result<Forward, Error>>),
}

struct Live {
    /// Waiting for [`Command::Approve`] of this hello: no request goes out and no pushed event is delivered.
    held: Option<Hello>,
    ws: Ws,
    next_id: RequestId,
    pending: HashMap<RequestId, (Instant, Waiter)>,
    /// Requests timed for the debug log (empty while debug mode is off).
    timing: HashMap<RequestId, skyelab_core::debug::RpcCall>,
    /// When to refill the stale threads, and the delay after that (doubles while fills keep failing).
    retry_at: Option<Instant>,
    retry_delay: Duration,
    /// Dropped with this connection: every forward started on it stops.
    session_ended: tokio::sync::watch::Sender<()>,
}

impl Live {
    async fn serve(&mut self, cx: &mut Ctx) -> End {
        for w in cx.watched.values_mut() {
            w.filling = false;
            w.stale = false;
            w.ahead.clear();
        }
        if self.held.is_none()
            && let Err(end) = self.resync(cx).await
        {
            return end;
        }
        let ping_every = cx.options.ping_interval;
        let mut ping = tokio::time::interval_at(Instant::now() + ping_every, ping_every);
        let mut heard_at = Instant::now();
        loop {
            let step = tokio::select! {
                // The ping check goes last: after this thread stalls, a frame that arrived meanwhile must update
                // heard_at before the overdue tick judges the server silent.
                biased;
                command = cx.commands.recv_async() => match command {
                    Err(_) => return End::Stopped,
                    Ok(command) => self.command(cx, command).await,
                },
                frame = self.ws.next() => {
                    heard_at = Instant::now();
                    match frame {
                        Some(Ok(Message::Text(text))) => match serde_json::from_str(&text) {
                            Ok(msg) => self.message(cx, msg).await,
                            Err(e) => Err(lost(format!("malformed message from the environment: {e}"))),
                        },
                        Some(Ok(Message::Binary(_))) => Err(lost("the environment sent a binary frame")),
                        Some(Ok(Message::Close(_))) | None => Err(lost("the environment closed the connection")),
                        Some(Ok(_)) => Ok(()), // ping/pong; tungstenite answers pings itself
                        // Oversized messages and protocol violations land here.
                        Some(Err(e)) => Err(lost(format!("connection error: {e}"))),
                    }
                }
                () = sleep_until(self.retry_at.unwrap_or_else(Instant::now)), if self.retry_at.is_some() => {
                    self.retry_at = None;
                    self.retry_stale(cx).await
                }
                now = ping.tick() => {
                    if now.duration_since(heard_at) > ping_every.saturating_mul(2) {
                        Err(lost("the environment stopped answering"))
                    } else {
                        self.expire(cx, now);
                        self.write(Message::Ping(Default::default()), ping_every).await
                    }
                }
            };
            if let Err(end) = step {
                return end;
            }
        }
    }

    /// Fills every watched thread from its last delivered seq.
    async fn resync(&mut self, cx: &mut Ctx) -> Result<(), End> {
        let threads: Vec<i64> = cx.watched.keys().copied().collect();
        for thread_id in threads {
            self.fill(cx, thread_id).await?;
        }
        Ok(())
    }

    async fn command(&mut self, cx: &mut Ctx, command: Command) -> Result<(), End> {
        match command {
            Command::Call { reply, .. } if self.held.is_some() => {
                reply(Err(Error::NotConnected));
                Ok(())
            }
            Command::Call { call, reply } => self.request(cx, call, Waiter::User(reply)).await,
            Command::Watch { id, after_seq } => {
                cx.watch(id, after_seq);
                if self.held.is_some() {
                    return Ok(()); // filled on approval
                }
                self.fill(cx, id.thread_id).await
            }
            Command::Unwatch(id) => {
                cx.unwatch(id);
                Ok(())
            }
            Command::Approve(hello) if self.held.as_ref() == Some(&hello) => {
                self.held = None;
                self.resync(cx).await
            }
            Command::Approve(_) => Ok(()), // another connection's hello, or already approved
            Command::Forward { reply, .. } if self.held.is_some() => {
                let _ = reply.send(Err(Error::NotConnected));
                Ok(())
            }
            Command::Forward { port, reply } => self.forward(cx, port, reply).await,
        }
    }

    async fn message(&mut self, cx: &mut Ctx, msg: ServerMessage) -> Result<(), End> {
        // Held: nothing was asked, and pushes from an unapproved server are dropped (the fill on approval
        // fetches them from the store).
        if self.held.is_some() {
            return Ok(());
        }
        let (id, result) = match msg {
            ServerMessage::Event(ev) => {
                let thread_id = ev.thread_id;
                let Some(w) = cx.watched.get_mut(&thread_id) else {
                    return Ok(());
                };
                return if w.accept(ev, &cx.updates) {
                    self.fill(cx, thread_id).await
                } else {
                    Ok(())
                };
            }
            ServerMessage::Response { id, result } => (id, Ok(result)),
            ServerMessage::Error {
                id: Some(id),
                error,
            } => (id, Err(Error::Rpc(error))),
            ServerMessage::Error { id: None, error } => {
                eprintln!("skyelab-client: the environment rejected a message: {error:?}");
                return Ok(());
            }
        };
        if let Some(call) = self.timing.remove(&id) {
            call.done(result.as_ref().err().map(ToString::to_string).as_deref());
        }
        match self.pending.remove(&id) {
            Some((_, Waiter::User(reply))) => {
                reply(result);
                Ok(())
            }
            Some((_, Waiter::Fill(thread_id))) => self.filled(cx, thread_id, result).await,
            Some((_, Waiter::Forward(port, reservation, reply))) => {
                self.forwarded(cx, port, reservation, reply, result);
                Ok(())
            }
            None => Ok(()), // timed out already, or an id we never sent
        }
    }

    /// Asks for the events after the thread's last delivered seq, unless a fill is already running.
    async fn fill(&mut self, cx: &mut Ctx, thread_id: i64) -> Result<(), End> {
        let Some(w) = cx.watched.get_mut(&thread_id) else {
            return Ok(());
        };
        if w.filling {
            return Ok(());
        }
        w.filling = true;
        let call = Call::ThreadEventsSince {
            thread_id,
            seq: w.last,
        };
        self.request(cx, call, Waiter::Fill(thread_id)).await
    }

    /// One page arrived. Pages while pages make progress; if a gap is left after that (the fill failed, or raced a
    /// push, or the server's store has a hole), retries after `backoff_min`.
    async fn filled(
        &mut self,
        cx: &mut Ctx,
        thread_id: i64,
        result: Result<Reply, Error>,
    ) -> Result<(), End> {
        let Some(w) = cx.watched.get_mut(&thread_id) else {
            return Ok(());
        };
        w.filling = false;
        let page = match result {
            Ok(Reply::Events(page)) => page,
            other => {
                eprintln!("skyelab-client: can't fetch events of thread {thread_id}: {other:?}");
                self.mark_stale(cx, thread_id);
                return Ok(());
            }
        };
        let before = w.last;
        let mut gap = !w.ahead.is_empty();
        for ev in page {
            if ev.thread_id == thread_id {
                gap = w.accept(ev, &cx.updates);
            }
        }
        if w.last > before {
            self.retry_delay = cx.options.backoff_min;
            self.fill(cx, thread_id).await
        } else {
            if gap {
                self.mark_stale(cx, thread_id);
            }
            Ok(())
        }
    }

    fn mark_stale(&mut self, cx: &mut Ctx, thread_id: i64) {
        if let Some(w) = cx.watched.get_mut(&thread_id) {
            w.stale = true;
            let delay = self.retry_delay;
            self.retry_at.get_or_insert_with(|| Instant::now() + delay);
        }
    }

    async fn retry_stale(&mut self, cx: &mut Ctx) -> Result<(), End> {
        self.retry_delay = self
            .retry_delay
            .saturating_mul(2)
            .min(cx.options.backoff_max);
        let stale: Vec<i64> = cx
            .watched
            .iter_mut()
            .filter(|(_, w)| w.stale)
            .map(|(id, w)| {
                w.stale = false;
                *id
            })
            .collect();
        for thread_id in stale {
            self.fill(cx, thread_id).await?;
        }
        Ok(())
    }

    async fn request(&mut self, cx: &Ctx, call: Call, waiter: Waiter) -> Result<(), End> {
        let id = self.next_id;
        self.next_id += 1;
        let text = serde_json::to_string(&Request { id, call })
            .unwrap_or_else(|_| unreachable!("Request always serializes"));
        let deadline = Instant::now() + cx.options.request_timeout;
        if let Some(call) = skyelab_core::debug::RpcCall::start("client", &text) {
            self.timing.insert(id, call);
        }
        // In the table first, so a failed send still answers the caller (ConnectionLost).
        self.pending.insert(id, (deadline, waiter));
        self.write(Message::text(text), cx.options.ping_interval)
            .await
    }

    /// A server that stops reading would block the send forever; past `limit` the connection is dropped.
    async fn write(&mut self, msg: Message, limit: Duration) -> Result<(), End> {
        match timeout(limit, self.ws.send(msg)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(lost(format!("send failed: {e}"))),
            Err(_) => Err(lost("send stalled")),
        }
    }
}
