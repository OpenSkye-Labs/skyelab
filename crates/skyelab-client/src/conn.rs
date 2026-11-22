//! The tokio side: one task owns the connection, the request table and every watched thread's seq.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use rustls::ClientConfig;
use skyelab_protocol::{Call, Hello, Reply, Request, RequestId, ServerMessage, Version};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::{Instant, sleep_until, timeout};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;

use crate::{Disconnect, DisconnectKind, Error, Options, Update, WatchId};

mod connect;
mod watched;
use connect::{Target, connect};
use watched::Watched;

/// The longest wait between tries while the machine hasn't approved this device yet.
const NOT_APPROVED_RETRY: Duration = Duration::from_secs(3);

pub(crate) type ReplyFn = Box<dyn FnOnce(Result<Reply, Error>) + Send>;

pub(crate) enum Command {
    Call { call: Call, reply: ReplyFn },
    Watch { id: WatchId, after_seq: i64 },
    Unwatch(WatchId),
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
    tls: Arc<ClientConfig>,
    commands: flume::Receiver<Command>,
    updates: flume::Sender<Update>,
    hello: Arc<Mutex<Option<Hello>>>,
    watched: HashMap<i64, Watched>,
}

impl Ctx {
    fn set_hello(&self, hello: Option<Hello>) {
        *self.hello.lock().unwrap_or_else(PoisonError::into_inner) = hello;
    }

    /// A command while there is no connection. Returns false once the client is gone.
    fn offline(&mut self, command: Result<Command, flume::RecvError>) -> bool {
        match command {
            Err(_) => return false,
            Ok(Command::Call { reply, .. }) => reply(Err(Error::NotConnected)),
            Ok(Command::Watch { id, after_seq }) => self.watch(id, after_seq),
            Ok(Command::Unwatch(id)) => self.unwatch(id),
        }
        true
    }

    /// Starts the thread over from `after_seq`, keeping its other watches.
    fn watch(&mut self, id: WatchId, after_seq: i64) {
        let mut w = Watched::new(after_seq);
        if let Some(old) = self.watched.remove(&id.thread_id) {
            w.holders = old.holders;
        }
        w.holders.insert(id.n);
        self.watched.insert(id.thread_id, w);
    }

    fn unwatch(&mut self, id: WatchId) {
        if let Some(w) = self.watched.get_mut(&id.thread_id) {
            w.holders.remove(&id.n);
            if w.holders.is_empty() {
                self.watched.remove(&id.thread_id);
            }
        }
    }

    fn allow_loopback(&self) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        return self.options.allow_loopback;
        #[cfg(not(any(test, feature = "test-support")))]
        false
    }

    fn plaintext(&self) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        return self.options.plaintext;
        #[cfg(not(any(test, feature = "test-support")))]
        false
    }

    fn resolve_to(&self) -> Option<std::net::IpAddr> {
        #[cfg(any(test, feature = "test-support"))]
        return self.options.resolve_to;
        #[cfg(not(any(test, feature = "test-support")))]
        None
    }

    fn protocol(&self) -> Version {
        #[cfg(any(test, feature = "test-support"))]
        return self.options.protocol;
        #[cfg(not(any(test, feature = "test-support")))]
        skyelab_protocol::VERSION
    }
}

pub(crate) async fn run(
    host: String,
    options: Options,
    tls: Arc<ClientConfig>,
    commands: flume::Receiver<Command>,
    updates: flume::Sender<Update>,
    hello: Arc<Mutex<Option<Hello>>>,
) {
    let mut cx = Ctx {
        host,
        options,
        tls,
        commands,
        updates,
        hello,
        watched: HashMap::new(),
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
    let target = Target {
        host: cx.host.clone(),
        options: cx.options.clone(),
        tls: cx.tls.clone(),
        allow_loopback: cx.allow_loopback(),
        plaintext: cx.plaintext(),
        resolve_to: cx.resolve_to(),
        protocol: cx.protocol(),
    };
    let connecting = timeout(cx.options.connect_timeout, connect(target));
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
    let _ = cx.updates.send(Update::Connected(hello));
    let mut live = Live {
        ws,
        next_id: 1,
        pending: HashMap::new(),
        retry_at: None,
        retry_delay: cx.options.backoff_min,
    };
    let end = live.serve(cx).await;
    // Whatever was in flight on this connection can't be answered now.
    for (_, (_, waiter)) in live.pending.drain() {
        if let Waiter::User(reply) = waiter {
            reply(Err(Error::ConnectionLost));
        }
    }
    let _ = timeout(Duration::from_secs(1), live.ws.close(None)).await;
    match end {
        End::Lost { why, .. } => End::Lost { why, was_up: true },
        end => end,
    }
}

enum Waiter {
    User(ReplyFn),
    Fill(i64),
}

struct Live {
    ws: Ws,
    next_id: RequestId,
    pending: HashMap<RequestId, (Instant, Waiter)>,
    /// When to refill the stale threads, and the delay after that (doubles while fills keep failing).
    retry_at: Option<Instant>,
    retry_delay: Duration,
}

impl Live {
    async fn serve(&mut self, cx: &mut Ctx) -> End {
        for w in cx.watched.values_mut() {
            w.filling = false;
            w.stale = false;
            w.ahead.clear();
        }
        let threads: Vec<i64> = cx.watched.keys().copied().collect();
        for thread_id in threads {
            if let Err(end) = self.fill(cx, thread_id).await {
                return end;
            }
        }
        let ping_every = cx.options.ping_interval;
        let mut ping = tokio::time::interval_at(Instant::now() + ping_every, ping_every);
        let mut heard_at = Instant::now();
        loop {
            let step = tokio::select! {
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

    async fn command(&mut self, cx: &mut Ctx, command: Command) -> Result<(), End> {
        match command {
            Command::Call { call, reply } => self.request(cx, call, Waiter::User(reply)).await,
            Command::Watch { id, after_seq } => {
                cx.watch(id, after_seq);
                self.fill(cx, id.thread_id).await
            }
            Command::Unwatch(id) => {
                cx.unwatch(id);
                Ok(())
            }
        }
    }

    async fn message(&mut self, cx: &mut Ctx, msg: ServerMessage) -> Result<(), End> {
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
        match self.pending.remove(&id) {
            Some((_, Waiter::User(reply))) => {
                reply(result);
                Ok(())
            }
            Some((_, Waiter::Fill(thread_id))) => self.filled(cx, thread_id, result).await,
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

    fn expire(&mut self, cx: &mut Ctx, now: Instant) {
        let expired: Vec<RequestId> = self
            .pending
            .iter()
            .filter(|(_, (deadline, _))| *deadline <= now)
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            match self.pending.remove(&id) {
                Some((_, Waiter::User(reply))) => reply(Err(Error::Timeout)),
                Some((_, Waiter::Fill(thread_id))) => {
                    if let Some(w) = cx.watched.get_mut(&thread_id) {
                        w.filling = false;
                    }
                    self.mark_stale(cx, thread_id);
                }
                None => {}
            }
        }
    }
}
