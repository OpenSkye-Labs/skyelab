//! Port forwarding (protocol 1.5): a listener on this machine's 127.0.0.1 whose every connection rides a wss
//! connection of its own to the environment, through its full gate, into `open_tunnel` on the forwarded port.

use std::collections::HashMap;
use std::net::{
    Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener as StdListener, TcpStream as StdStream,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use skyelab_protocol::{Call, Reply, Request, ServerMessage};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, watch};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

use super::connect::{Target, connect};
use super::{Command, Ws};

/// The daemon gates at most 4 connections per address at a time and drops the rest, and a browser opens about 6
/// at once; one client dials at most this many tunnels at a time, across all its forwards.
pub(crate) const MAX_DIALS: usize = 3;
const MAX_TUNNELS: usize = 64;

/// Which remote ports this client forwards, each with the id of the `Forward` holding it. The daemon keys a
/// connection's forwards by port, so a second forward of one port would share the first one's token and its stop.
#[derive(Clone, Default)]
pub(crate) struct Forwarding(Arc<Mutex<HashMap<u16, u64>>>);

static NEXT_FORWARD: AtomicU64 = AtomicU64::new(1);

impl Forwarding {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u16, u64>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Reserves `port`, or `None` when a forward already holds it.
    pub(super) fn claim(&self, port: u16) -> Option<u64> {
        let mut held = self.lock();
        if held.contains_key(&port) {
            return None;
        }
        let id = NEXT_FORWARD.fetch_add(1, Ordering::Relaxed);
        held.insert(port, id);
        Some(id)
    }

    /// Frees `port` if reservation `id` still holds it (a later forward of the port keeps its own).
    pub(super) fn release(&self, port: u16, id: u64) {
        let mut held = self.lock();
        if held.get(&port) == Some(&id) {
            held.remove(&port);
        }
    }

    /// The connection ended, and every forward with it.
    pub(super) fn clear(&self) {
        self.lock().clear();
    }
}

/// A forwarded port: connections to `127.0.0.1:local_port` here reach `remote_port` on the environment's loopback.
/// Dropping it closes the listener and its tunnels and tells the environment to stop. It also stops when the
/// connection to the environment drops; forward again after a reconnect.
pub struct Forward {
    pub remote_port: u16,
    pub local_port: u16,
    _stop_on_drop: watch::Sender<()>,
    token: String,
    reservation: u64,
    commands: flume::WeakSender<Command>,
    forwarding: Forwarding,
}

impl std::fmt::Debug for Forward {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Forward")
            .field("remote_port", &self.remote_port)
            .field("local_port", &self.local_port)
            .finish()
    }
}

impl Drop for Forward {
    fn drop(&mut self) {
        self.forwarding.release(self.remote_port, self.reservation);
        stop_remote(&self.commands, self.remote_port, &self.token);
    }
}

/// Sends `stop_forward` for the forward `token` names, if the client is still running. A weak sender: a forward
/// never keeps a dropped client's connection alive.
pub(super) fn stop_remote(commands: &flume::WeakSender<Command>, port: u16, token: &str) {
    if let Some(commands) = commands.upgrade() {
        let token = token.to_owned();
        let _ = commands.send(Command::Call {
            call: Box::new(Call::StopForward { port, token }),
            reply: Box::new(|_| {}),
        });
    }
}

/// True when something here already answers on `port` at 127.0.0.1 or ::1. Binding can't tell: std sets
/// SO_REUSEADDR, and on macOS that lets 127.0.0.1:P bind over a local server on the wildcard address.
fn answers_here(port: u16) -> bool {
    [Ipv4Addr::LOCALHOST.into(), Ipv6Addr::LOCALHOST.into()]
        .into_iter()
        .any(|ip| {
            StdStream::connect_timeout(&SocketAddr::new(ip, port), Duration::from_millis(200))
                .is_ok()
        })
}

/// 127.0.0.1 on `port` when nothing here answers on it, else any free port. Never a wildcard or a tailnet address.
pub(crate) fn bind_local(port: u16) -> std::io::Result<StdListener> {
    // A child forked mid-bind would keep the listener (and the port) after the forward stops.
    let bind = |port: u16| {
        skyelab_core::process::exclusive(|| {
            // Windows lets a same-user socket with SO_REUSEADDR bind the port over a plain listener.
            #[cfg(windows)]
            return skyelab_net_sys::bind_exclusive((Ipv4Addr::LOCALHOST, port).into());
            #[cfg(not(windows))]
            StdListener::bind((Ipv4Addr::LOCALHOST, port))
        })
    };
    let listener = match answers_here(port) {
        true => bind(0),
        false => bind(port).or_else(|_| bind(0)),
    }?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

/// What a forward's tunnels dial with.
pub(super) struct Route {
    pub(super) target: Target,
    /// The node the approved main connection named; every tunnel must reach the same one.
    pub(super) approved_node: Option<String>,
    pub(super) remote_port: u16,
    pub(super) token: String,
    pub(super) reservation: u64,
    /// Errors once the main connection has ended.
    pub(super) session_ended: watch::Receiver<()>,
    pub(super) dials: Arc<Semaphore>,
    pub(super) commands: flume::WeakSender<Command>,
    pub(super) forwarding: Forwarding,
}

/// Both ways a forward stops: its `Forward` dropped, or the main connection ended. Nothing is ever sent on either
/// channel; each resolves when its sender drops.
#[derive(Clone)]
struct Stops(watch::Receiver<()>, watch::Receiver<()>);

impl Stops {
    async fn fired(&mut self) {
        tokio::select! {
            () = async { while self.0.changed().await.is_ok() {} } => {}
            () = async { while self.1.changed().await.is_ok() {} } => {}
        }
    }
}

/// Serves `listener` on the current runtime until the forward is dropped or the session ends.
pub(super) fn start(listener: StdListener, route: Route) -> std::io::Result<Forward> {
    let local_port = listener.local_addr()?.port();
    let listener = TcpListener::from_std(listener)?;
    let (stop, stopped) = watch::channel(());
    let forward = Forward {
        remote_port: route.remote_port,
        local_port,
        _stop_on_drop: stop,
        token: route.token.clone(),
        reservation: route.reservation,
        commands: route.commands.clone(),
        forwarding: route.forwarding.clone(),
    };
    let mut stops = Stops(stopped, route.session_ended.clone());
    let route = Arc::new(route);
    tokio::spawn(async move {
        let open = Arc::new(Semaphore::new(MAX_TUNNELS));
        loop {
            let accepted = tokio::select! {
                () = stops.fired() => return,
                accepted = listener.accept() => accepted,
            };
            let tcp = match accepted {
                Ok((tcp, _)) => tcp,
                Err(_) => {
                    // Out of descriptors and the like: back off instead of spinning.
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };
            // Past the cap the connection is closed at once.
            let Ok(slot) = open.clone().try_acquire_owned() else {
                continue;
            };
            let (route, mut stops, pump_stops) = (route.clone(), stops.clone(), stops.clone());
            tokio::spawn(async move {
                tokio::select! {
                    () = stops.fired() => {}
                    () = tunnel(&route, tcp, pump_stops) => {}
                }
                drop(slot);
            });
        }
    });
    Ok(forward)
}

/// One local connection: a new gated connection to the environment, `open_tunnel`, then bytes both ways.
async fn tunnel(route: &Route, tcp: TcpStream, stops: Stops) {
    let Ok(dialling) = route.dials.acquire().await else {
        return;
    };
    let limit = route.target.options.connect_timeout;
    let Ok(Ok((mut ws, hello))) = timeout(limit, connect(route.target.clone())).await else {
        return;
    };
    if hello.node_id != route.approved_node {
        return;
    }
    let call = Call::OpenTunnel {
        port: route.remote_port,
        token: route.token.clone(),
    };
    let text = serde_json::to_string(&Request { id: 1, call })
        .unwrap_or_else(|_| unreachable!("Request always serializes"));
    if !matches!(
        timeout(limit, ws.send(Message::text(text))).await,
        Ok(Ok(()))
    ) {
        return;
    }
    let admitted = loop {
        match timeout(limit, ws.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => match serde_json::from_str(&text) {
                Ok(ServerMessage::Response {
                    id: 1,
                    result: Reply::Ok,
                }) => break true,
                // A tunnel gets no pushes from a 1.5 daemon; skip any rather than drop the browser's connection.
                Ok(ServerMessage::Event(_)) => continue,
                _ => break false,
            },
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => continue,
            _ => break false,
        }
    };
    drop(dialling);
    if admitted {
        let _ = tcp.set_nodelay(true);
        pump(ws, tcp, stops, route.target.options.request_timeout).await;
    }
}

/// Copies bytes between `ws` and `tcp` until either closes or a stop fires. A write stalled for `limit` ends it.
// Same loop as the daemon's (skyelab-daemon forward.rs `pump`): the crates share no runtime code.
async fn pump(ws: Ws, tcp: TcpStream, mut stops: Stops, limit: Duration) {
    // shortcut: EOF on either side closes the whole tunnel (no TCP half-close). Fine for HTTP and WebSocket; carry
    // half-close if a forwarded protocol needs it.
    let (mut sink, mut stream) = ws.split();
    let (mut read, mut write) = tcp.into_split();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let open = tokio::select! {
            () = stops.fired() => false,
            frame = stream.next() => match frame {
                Some(Ok(Message::Binary(data))) => tokio::select! {
                    () = stops.fired() => false,
                    wrote = timeout(limit, write.write_all(&data)) => matches!(wrote, Ok(Ok(()))),
                },
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => true,
                _ => false,
            },
            n = read.read(&mut buf) => match n {
                Ok(0) | Err(_) => false,
                Ok(n) => tokio::select! {
                    () = stops.fired() => false,
                    sent = timeout(limit, sink.send(Message::binary(buf[..n].to_vec()))) => matches!(sent, Ok(Ok(()))),
                },
            },
        };
        if !open {
            break;
        }
    }
    let _ = timeout(Duration::from_secs(1), sink.close()).await;
}
