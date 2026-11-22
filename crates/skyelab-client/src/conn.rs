//! The tokio side: one task owns the connection, the request table and every watched thread's seq.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use skyelab_protocol::{
    Call, ErrorCode, Hello, Reply, Request, RequestId, ServerMessage, ThreadEvent, Version,
};
use tokio::net::TcpStream;
use tokio::time::{Instant, sleep_until, timeout};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use crate::{Error, Options, Update, guard};

pub(crate) type ReplyFn = Box<dyn FnOnce(Result<Reply, Error>) + Send>;

pub(crate) enum Command {
    Call { call: Call, reply: ReplyFn },
    Watch { thread_id: i64, after_seq: i64 },
    Unwatch { thread_id: i64 },
}

/// Out-of-order pushes kept per thread while a fill runs. Past it they're dropped; the fill fetches them anyway.
const MAX_AHEAD: usize = 1000;

struct Watched {
    /// The last seq delivered.
    last: i64,
    filling: bool,
    /// Pushed events past a gap, waiting for the fill to reach them.
    ahead: BTreeMap<i64, ThreadEvent>,
}

impl Watched {
    fn new(last: i64) -> Self {
        Self {
            last,
            filling: false,
            ahead: BTreeMap::new(),
        }
    }

    /// Delivers `ev` if it is the next seq (then any buffered ones that follow), drops it if already delivered,
    /// buffers it if there is a gap. Every event goes through here, pushed or fetched, so the order can't matter.
    fn accept(&mut self, ev: ThreadEvent, updates: &flume::Sender<Update>) {
        if ev.seq <= self.last {
            return;
        }
        if ev.seq > self.last + 1 {
            if self.ahead.len() < MAX_AHEAD {
                self.ahead.insert(ev.seq, ev);
            } else {
                self.ahead.clear();
            }
            return;
        }
        self.last = ev.seq;
        let _ = updates.send(Update::Event(ev));
        self.ahead = self.ahead.split_off(&(self.last + 1));
        while let Some(next) = self.ahead.remove(&(self.last + 1)) {
            self.last = next.seq;
            let _ = updates.send(Update::Event(next));
        }
    }
}

enum End {
    /// The client was dropped.
    Stopped,
    Fatal(Error),
    /// Retry. `was_up`: the connection got past hello, so the backoff starts over.
    Lost {
        reason: String,
        was_up: bool,
    },
}

fn lost(reason: impl Into<String>) -> End {
    End::Lost {
        reason: reason.into(),
        was_up: false,
    }
}

struct Ctx {
    host: String,
    options: Options,
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
            Ok(Command::Watch {
                thread_id,
                after_seq,
            }) => {
                self.watched.insert(thread_id, Watched::new(after_seq));
            }
            Ok(Command::Unwatch { thread_id }) => {
                self.watched.remove(&thread_id);
            }
        }
        true
    }

    fn allow_loopback(&self) -> bool {
        #[cfg(test)]
        return self.options.allow_loopback;
        #[cfg(not(test))]
        false
    }

    fn protocol(&self) -> Version {
        #[cfg(test)]
        return self.options.protocol;
        #[cfg(not(test))]
        skyelab_protocol::VERSION
    }
}

pub(crate) async fn run(
    host: String,
    options: Options,
    commands: flume::Receiver<Command>,
    updates: flume::Sender<Update>,
    hello: Arc<Mutex<Option<Hello>>>,
) {
    let mut cx = Ctx {
        host,
        options,
        commands,
        updates,
        hello,
        watched: HashMap::new(),
    };
    let mut backoff = cx.options.backoff_min;
    loop {
        let (reason, was_up) = match session(&mut cx).await {
            End::Stopped => return,
            End::Fatal(e) => {
                let _ = cx.updates.send(Update::Fatal(e));
                return;
            }
            End::Lost { reason, was_up } => (reason, was_up),
        };
        cx.set_hello(None);
        let _ = cx.updates.send(Update::Disconnected(reason));
        if was_up {
            backoff = cx.options.backoff_min;
        }
        let retry_at = Instant::now() + backoff;
        loop {
            tokio::select! {
                () = sleep_until(retry_at) => break,
                command = cx.commands.recv_async() => if !cx.offline(command) { return },
            }
        }
        backoff = (backoff * 2).min(cx.options.backoff_max);
    }
}

type Ws = WebSocketStream<TcpStream>;

async fn session(cx: &mut Ctx) -> End {
    let target = Target {
        host: cx.host.clone(),
        options: cx.options.clone(),
        allow_loopback: cx.allow_loopback(),
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
        End::Lost { reason, .. } => End::Lost {
            reason,
            was_up: true,
        },
        end => end,
    }
}

/// Resolve, refuse non-tailnet addresses, dial the checked addresses, handshake, hello.
struct Target {
    host: String,
    options: Options,
    allow_loopback: bool,
    protocol: Version,
}

async fn connect(cx: Target) -> Result<(Ws, Hello), End> {
    let port = cx.options.port;
    let addrs = guard::resolve(&cx.host, port, cx.allow_loopback)
        .await
        .map_err(lost)?;
    let stream = dial(&addrs).await.map_err(lost)?;
    let limit = cx.options.max_message_bytes;
    let config = WebSocketConfig::default()
        .max_message_size(Some(limit))
        .max_frame_size(Some(limit));
    // The URL only names the Host header; the socket is already connected to a checked address.
    let host = if cx.host.contains(':') {
        format!("[{}]", cx.host)
    } else {
        cx.host.clone()
    };
    let (mut ws, _) = tokio_tungstenite::client_async_with_config(
        format!("ws://{host}:{port}/"),
        stream,
        Some(config),
    )
    .await
    .map_err(|e| lost(format!("WebSocket handshake failed: {e}")))?;

    let hello = Request {
        id: 0,
        call: Call::Hello {
            protocol: cx.protocol,
            capabilities: vec![],
        },
    };
    send_json(&mut ws, &hello).await.map_err(lost)?;
    loop {
        let text = match ws.next().await {
            Some(Ok(Message::Text(text))) => text,
            Some(Ok(Message::Close(_))) | None => return Err(lost("closed during hello")),
            Some(Ok(Message::Binary(_))) => return Err(lost("binary frame during hello")),
            Some(Ok(_)) => continue,
            Some(Err(e)) => return Err(lost(format!("hello failed: {e}"))),
        };
        return match serde_json::from_str::<ServerMessage>(&text) {
            Ok(ServerMessage::Response {
                id: 0,
                result: Reply::Hello(h),
            }) if h.protocol.major == cx.protocol.major => Ok((ws, h)),
            Ok(ServerMessage::Response {
                id: 0,
                result: Reply::Hello(h),
            }) => Err(End::Fatal(Error::UnsupportedVersion(format!(
                "the environment answered with protocol {}.{}",
                h.protocol.major, h.protocol.minor
            )))),
            Ok(ServerMessage::Error { error, .. })
                if error.code == ErrorCode::UnsupportedVersion =>
            {
                Err(End::Fatal(Error::UnsupportedVersion(error.message)))
            }
            Ok(other) => Err(lost(format!("unexpected answer to hello: {other:?}"))),
            Err(e) => Err(lost(format!("malformed answer to hello: {e}"))),
        };
    }
}

async fn dial(addrs: &[SocketAddr]) -> Result<TcpStream, String> {
    let mut errors = Vec::new();
    for addr in addrs {
        match TcpStream::connect(addr).await {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            Err(e) => errors.push(format!("{addr}: {e}")),
        }
    }
    Err(format!("can't connect: {}", errors.join("; ")))
}

async fn send_json(ws: &mut Ws, request: &Request) -> Result<(), String> {
    // Plain structs, strings and integers: serializing can't fail.
    let text = serde_json::to_string(request)
        .unwrap_or_else(|_| unreachable!("Request always serializes"));
    ws.send(Message::text(text))
        .await
        .map_err(|e| format!("send failed: {e}"))
}

enum Waiter {
    User(ReplyFn),
    Fill(i64),
}

struct Live {
    ws: Ws,
    next_id: RequestId,
    pending: HashMap<RequestId, (Instant, Waiter)>,
}

impl Live {
    async fn serve(&mut self, cx: &mut Ctx) -> End {
        for w in cx.watched.values_mut() {
            w.filling = false;
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
                now = ping.tick() => {
                    if now.duration_since(heard_at) > ping_every * 2 {
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
            Command::Watch {
                thread_id,
                after_seq,
            } => {
                cx.watched.insert(thread_id, Watched::new(after_seq));
                self.fill(cx, thread_id).await
            }
            Command::Unwatch { thread_id } => {
                cx.watched.remove(&thread_id);
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
                w.accept(ev, &cx.updates);
                return if w.ahead.is_empty() {
                    Ok(())
                } else {
                    self.fill(cx, thread_id).await
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
            None => Ok(()), // timed out already
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

    /// One page arrived. Pages until one is empty or makes no progress (a hole in the server's store).
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
                return Ok(());
            }
        };
        let before = w.last;
        for ev in page {
            if ev.thread_id == thread_id {
                w.accept(ev, &cx.updates);
            }
        }
        if w.last > before {
            self.fill(cx, thread_id).await
        } else {
            Ok(())
        }
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
                }
                None => {}
            }
        }
    }
}
