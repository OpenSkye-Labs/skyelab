//! Port forwarding (protocol 1.5): an approved device tunnels to a port on this machine's loopback.
//!
//! Each tunnel is a connection of its own through the same gate. Its sender lives in the owner's peer entry, so a
//! tunnel ends with the forward: on `stop_forward`, or when the owner closes or is revoked.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Notify, watch};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

use crate::Ws;
use crate::peers::{Peers, lock_peers};

/// Forwards one connection may hold at once.
pub(crate) const MAX_FORWARDS: usize = 32;
/// Tunnels open at once through one forward.
pub(crate) const MAX_TUNNELS: usize = 64;
const DIAL_TIMEOUT: Duration = Duration::from_secs(5);

/// Services that hand whoever reaches them control of this machine or its user, with no login of their own.
const DENIED: &[(u16, &str)] = &[
    (2375, "Docker's API"),
    (2379, "etcd"),
    (5037, "the adb server"),
    (5432, "Postgres"),
    (6000, "an X11 display"),
    (8001, "kubectl proxy"),
    (11211, "memcached"),
    (27017, "MongoDB"),
    (2376, "Docker's API"),
    (3389, "Remote Desktop"),
    (5900, "Screen Sharing (VNC)"),
    (6379, "Redis"),
    (9222, "a browser's remote debugging port"),
    (9229, "Node's inspector"),
];

/// Why `port` can't be forwarded, or `None` when it can.
pub(crate) fn refusal(port: u16, own_ports: &[u16]) -> Option<String> {
    if port < 1024 {
        return Some(format!(
            "port {port} is a system port; only ports 1024 and up can be forwarded"
        ));
    }
    if own_ports.contains(&port) {
        return Some(format!("port {port} is SkyeLab's own sharing port"));
    }
    DENIED
        .iter()
        .find(|(p, _)| *p == port)
        .map(|(_, what)| format!("port {port} is usually {what}, which SkyeLab never forwards"))
}

/// A connection to `port` on this machine's loopback: 127.0.0.1, else ::1. Never any other host.
pub(crate) async fn dial_loopback(port: u16) -> Result<TcpStream, String> {
    for ip in [Ipv4Addr::LOCALHOST.into(), Ipv6Addr::LOCALHOST.into()] {
        if let Ok(Ok(tcp)) =
            timeout(DIAL_TIMEOUT, TcpStream::connect(SocketAddr::new(ip, port))).await
        {
            let _ = tcp.set_nodelay(true);
            return Ok(tcp);
        }
    }
    Err(format!("nothing accepts connections on port {port} there"))
}

/// One forward a connection holds. Its tunnels watch `ended` and close when it drops.
pub(crate) struct Forward {
    /// Names this forward in `open_tunnel` and `stop_forward`, so a tunnel joins exactly the forward its client
    /// asked for (two windows of one device can forward the same port).
    token: String,
    ended: watch::Sender<()>,
}

/// Records that connection `peer` forwards `port` and returns its token (the same one if it already does). Fails
/// when the connection is gone, cut off, a tunnel, or holds too many.
pub(crate) fn allow(peers: &Peers, peer: u64, port: u16) -> Result<String, String> {
    let mut peers = lock_peers(peers);
    let Some(entry) = peers.get_mut(&peer) else {
        return Err("the connection closed".into());
    };
    if entry.gate.cancelled() || entry.tunnel {
        return Err("this connection can't forward ports".into());
    }
    if let Some(forward) = entry.forwards.get(&port) {
        return Ok(forward.token.clone());
    }
    if entry.forwards.len() >= MAX_FORWARDS {
        return Err(format!(
            "at most {MAX_FORWARDS} ports can be forwarded at once"
        ));
    }
    let token = skyelab_protocol::new_device_key().map_err(|e| e.to_string())?;
    entry.forwards.insert(
        port,
        Forward {
            token: token.clone(),
            ended: watch::channel(()).0,
        },
    );
    Ok(token)
}

/// Ends connection `peer`'s forward of `port` if `token` names it; its tunnels see the sender drop and close.
pub(crate) fn stop(peers: &Peers, peer: u64, port: u16, token: &str) {
    if let Some(entry) = lock_peers(peers).get_mut(&peer)
        && entry.forwards.get(&port).is_some_and(|f| f.token == token)
    {
        entry.forwards.remove(&port);
    }
}

/// Makes tunnel connection `me` a tunnel of the forward `token` names, held by another live, non-tunnel
/// connection with the same approval.
pub(crate) fn join(
    peers: &Peers,
    me: u64,
    port: u16,
    token: &str,
) -> Result<watch::Receiver<()>, String> {
    let peers = lock_peers(peers);
    let approval = peers
        .get(&me)
        .and_then(|e| e.approval.clone())
        .ok_or("this device isn't approved")?;
    let ended = &peers
        .iter()
        .filter(|(id, e)| {
            **id != me && !e.tunnel && e.approval.as_ref() == Some(&approval) && !e.gate.cancelled()
        })
        .find_map(|(_, e)| e.forwards.get(&port).filter(|f| f.token == token))
        .ok_or_else(|| format!("port {port} isn't forwarded to this device"))?
        .ended;
    // The forward's own sender has no receiver; each tunnel adds one.
    if ended.receiver_count() >= MAX_TUNNELS {
        return Err(format!(
            "port {port} already has {MAX_TUNNELS} open tunnels"
        ));
    }
    Ok(ended.subscribe())
}

/// Resolves once the connection is kicked or the forward ends (its sender dropped; nothing is ever sent on it).
async fn stopped(kick: &Notify, ended: &mut watch::Receiver<()>) {
    tokio::select! {
        () = kick.notified() => {}
        () = async { while ended.changed().await.is_ok() {} } => {}
    }
}

/// Copies bytes between the tunnel's WebSocket and `tcp` until either side closes, the forward ends, or the
/// connection is kicked. A write that stalls for `limit` ends it too.
// Same loop as the client's (skyelab-client conn/forward.rs `pump`): the crates share no runtime code.
pub(crate) async fn pump(
    mut sink: SplitSink<Ws, Message>,
    mut stream: SplitStream<Ws>,
    tcp: TcpStream,
    mut ended: watch::Receiver<()>,
    kick: &Notify,
    limit: Duration,
) {
    // shortcut: EOF on either side closes the whole tunnel (no TCP half-close). Fine for HTTP and WebSocket; carry
    // half-close if a forwarded protocol needs it.
    let (mut read, mut write) = tcp.into_split();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let open = tokio::select! {
            biased;
            () = stopped(kick, &mut ended) => false,
            frame = stream.next() => match frame {
                Some(Ok(Message::Binary(data))) => tokio::select! {
                    biased;
                    () = stopped(kick, &mut ended) => false,
                    wrote = timeout(limit, write.write_all(&data)) => matches!(wrote, Ok(Ok(()))),
                },
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => true,
                _ => false, // close, error, or a text frame, which a tunnel never carries
            },
            n = read.read(&mut buf) => match n {
                Ok(0) | Err(_) => false,
                Ok(n) => tokio::select! {
                    biased;
                    () = stopped(kick, &mut ended) => false,
                    sent = timeout(limit, sink.send(Message::binary(buf[..n].to_vec()))) => matches!(sent, Ok(Ok(()))),
                },
            },
        };
        if !open {
            break;
        }
    }
    // A peer that stopped reading must not hold the socket long.
    let _ = timeout(Duration::from_secs(1), sink.close()).await;
}
