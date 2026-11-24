//! Port forwarding (protocol 1.5): a listener on this machine's 127.0.0.1 whose every connection rides a wss
//! connection of its own to the environment, through its full gate, into `open_tunnel` on the forwarded port.

use std::collections::HashSet;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener as StdListener};
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

/// Tunnels being dialled at once. The daemon gates at most 4 connections per address at a time and drops the
/// rest, and a browser opens about 6 at once.
const MAX_DIALS: usize = 3;
/// Tunnels open at once per forward (the daemon's own limit is 64).
const MAX_TUNNELS: usize = 64;

/// Remote ports forwarded on this client, so a port is never forwarded twice (stopping one would end the other's
/// tunnels on the environment).
pub(crate) type Forwarding = Arc<Mutex<HashSet<u16>>>;

/// A forwarded port: connections to `127.0.0.1:local_port` here reach `remote_port` on the environment's loopback.
/// Dropping it closes the listener and its tunnels and tells the environment to stop. It also stops when the
/// connection to the environment drops; forward again after a reconnect.
pub struct Forward {
    pub remote_port: u16,
    pub local_port: u16,
    /// Dropped with the forward; the listener and every tunnel watch it.
    _stop: watch::Sender<()>,
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
        self.forwarding
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.remote_port);
        // A weak sender: a forward never keeps a dropped client's connection alive.
        if let Some(commands) = self.commands.upgrade() {
            let call = Call::StopForward {
                port: self.remote_port,
            };
            let _ = commands.send(Command::Call {
                call,
                reply: Box::new(|_| {}),
            });
        }
    }
}

/// 127.0.0.1 on `port` when it is free there and on ::1 (a browser's `localhost` may try ::1 first, and a server
/// there would answer instead of the forward), else any free port. Never a wildcard or a tailnet address.
pub(crate) fn bind_local(port: u16) -> std::io::Result<StdListener> {
    // A child forked mid-bind would keep the listener (and the port) after the forward stops.
    let bind = |addr: SocketAddr| skyelab_core::process::exclusive(|| StdListener::bind(addr));
    let v6_free = bind(SocketAddr::new(Ipv6Addr::LOCALHOST.into(), port)).is_ok();
    let listener = match v6_free {
        true => bind(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port)),
        false => Err(std::io::ErrorKind::AddrInUse.into()),
    }
    .or_else(|_| bind(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0)))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

/// What a forward's tunnels dial with.
pub(super) struct Route {
    pub(super) target: Target,
    /// The node the approved main connection named; every tunnel must reach the same one.
    pub(super) node: Option<String>,
    pub(super) remote_port: u16,
    /// Dropped when the main connection ends.
    pub(super) session: watch::Receiver<()>,
    pub(super) commands: flume::WeakSender<Command>,
    pub(super) forwarding: Forwarding,
}

/// Serves `listener` on the current runtime until the forward is dropped or the session ends.
pub(super) fn start(listener: StdListener, route: Route) -> std::io::Result<Forward> {
    let local_port = listener.local_addr()?.port();
    let listener = TcpListener::from_std(listener)?;
    let (stop, stopped) = watch::channel(());
    let forward = Forward {
        remote_port: route.remote_port,
        local_port,
        _stop: stop,
        commands: route.commands.clone(),
        forwarding: route.forwarding.clone(),
    };
    let route = Arc::new(route);
    tokio::spawn(async move {
        let dials = Arc::new(Semaphore::new(MAX_DIALS));
        let open = Arc::new(Semaphore::new(MAX_TUNNELS));
        let (mut stopped, mut session) = (stopped, route.session.clone());
        loop {
            tokio::select! {
                _ = stopped.changed() => return,
                _ = session.changed() => return,
                accepted = listener.accept() => {
                    let Ok((tcp, _)) = accepted else { continue };
                    // Past the cap the connection is refused; the browser retries.
                    let Ok(slot) = open.clone().try_acquire_owned() else { continue };
                    let (route, dials, stops) = (route.clone(), dials.clone(), (stopped.clone(), session.clone()));
                    tokio::spawn(async move {
                        tunnel(&route, tcp, &dials, stops).await;
                        drop(slot);
                    });
                }
            }
        }
    });
    Ok(forward)
}

/// One local connection: a new gated connection to the environment, `open_tunnel`, then bytes both ways.
async fn tunnel(
    route: &Route,
    tcp: TcpStream,
    dials: &Semaphore,
    stops: (watch::Receiver<()>, watch::Receiver<()>),
) {
    let Ok(dialling) = dials.acquire().await else {
        return;
    };
    let limit = route.target.options.connect_timeout;
    let Ok(Ok((mut ws, hello))) = timeout(limit, connect(route.target.clone())).await else {
        return;
    };
    // Another node behind the name now: not the machine the user approved.
    if hello.node_id != route.node {
        return;
    }
    let open = Request {
        id: 1,
        call: Call::OpenTunnel {
            port: route.remote_port,
        },
    };
    let text =
        serde_json::to_string(&open).unwrap_or_else(|_| unreachable!("Request always serializes"));
    if !matches!(
        timeout(limit, ws.send(Message::text(text))).await,
        Ok(Ok(()))
    ) {
        return;
    }
    let admitted = loop {
        match timeout(limit, ws.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                break matches!(
                    serde_json::from_str(&text),
                    Ok(ServerMessage::Response {
                        id: 1,
                        result: Reply::Ok
                    })
                );
            }
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
async fn pump(
    ws: Ws,
    tcp: TcpStream,
    (mut stopped, mut session): (watch::Receiver<()>, watch::Receiver<()>),
    limit: Duration,
) {
    // shortcut: EOF on either side closes the whole tunnel (no TCP half-close); fine for HTTP and WebSocket clients.
    let (mut sink, mut stream) = ws.split();
    let (mut read, mut write) = tcp.into_split();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        tokio::select! {
            _ = stopped.changed() => break,
            _ = session.changed() => break,
            frame = stream.next() => match frame {
                Some(Ok(Message::Binary(data))) => {
                    if !matches!(timeout(limit, write.write_all(&data)).await, Ok(Ok(()))) {
                        break;
                    }
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                _ => break,
            },
            n = read.read(&mut buf) => match n {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let sent = timeout(limit, sink.send(Message::binary(buf[..n].to_vec()))).await;
                    if !matches!(sent, Ok(Ok(()))) {
                        break;
                    }
                }
            },
        }
    }
    let _ = timeout(Duration::from_secs(1), sink.close()).await;
}
