//! Port forwarding (protocol 1.5): an approved device tunnels to a port on this machine's loopback.
//!
//! The main connection asks with `forward_port`. Each tunnel is a connection of its own through the same gate
//! (whois, TLS, approved key), and `open_tunnel` admits it only while a live connection with the same approval
//! forwards that port. The tunnel holds a receiver whose sender lives in that connection's peer entry, so it ends
//! with the forward: on `stop_forward`, or when the connection closes or is revoked.

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
    (2376, "Docker's API"),
    (3389, "Remote Desktop"),
    (5900, "Screen Sharing (VNC)"),
    (6379, "Redis"),
    (9222, "a browser's remote debugging port"),
    (9229, "Node's inspector"),
];

/// Why `port` can't be forwarded, or `None` when it can. `own` holds the daemon's listening ports.
pub(crate) fn refusal(port: u16, own: &[u16]) -> Option<String> {
    if port < 1024 {
        return Some(format!(
            "port {port} is a system port; only ports 1024 and up can be forwarded"
        ));
    }
    if own.contains(&port) {
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

/// Records that connection `peer` forwards `port`. Fails when the connection is gone, cut off, or holds too many.
pub(crate) fn allow(peers: &Peers, peer: u64, port: u16) -> Result<(), String> {
    let mut peers = lock_peers(peers);
    let Some(entry) = peers.get_mut(&peer) else {
        return Err("the connection closed".into());
    };
    if entry.gate.cancelled() {
        return Err("this device was cut off".into());
    }
    if !entry.forwards.contains_key(&port) && entry.forwards.len() >= MAX_FORWARDS {
        return Err(format!(
            "at most {MAX_FORWARDS} ports can be forwarded at once"
        ));
    }
    entry
        .forwards
        .entry(port)
        .or_insert_with(|| watch::channel(()).0);
    Ok(())
}

/// Ends connection `peer`'s forward of `port`; its tunnels see the sender drop and close.
pub(crate) fn stop(peers: &Peers, peer: u64, port: u16) {
    if let Some(entry) = lock_peers(peers).get_mut(&peer) {
        entry.forwards.remove(&port);
    }
}

/// Makes connection `me` a tunnel of a live forward of `port` by a connection with the same approval.
pub(crate) fn join(peers: &Peers, me: u64, port: u16) -> Result<watch::Receiver<()>, String> {
    let mut peers = lock_peers(peers);
    let approval = peers
        .get(&me)
        .and_then(|e| e.approval.clone())
        .ok_or("this device isn't approved")?;
    let sender = peers
        .iter()
        .filter(|(id, e)| {
            **id != me && e.approval.as_ref() == Some(&approval) && !e.gate.cancelled()
        })
        .find_map(|(_, e)| e.forwards.get(&port))
        .ok_or_else(|| format!("port {port} isn't forwarded to this device"))?;
    // The forward's own sender has no receiver; each tunnel adds one.
    if sender.receiver_count() >= MAX_TUNNELS {
        return Err(format!(
            "port {port} already has {MAX_TUNNELS} open tunnels"
        ));
    }
    let ended = sender.subscribe();
    if let Some(entry) = peers.get_mut(&me) {
        entry.tunnel = true;
    }
    Ok(ended)
}

/// Copies bytes between the tunnel's WebSocket and `tcp` until either side closes, the forward ends, or the
/// connection is kicked. A write that stalls for `limit` ends it too.
pub(crate) async fn pump(
    mut sink: SplitSink<Ws, Message>,
    mut stream: SplitStream<Ws>,
    tcp: TcpStream,
    mut ended: watch::Receiver<()>,
    kick: &Notify,
    limit: Duration,
) {
    // shortcut: EOF on either side closes the whole tunnel (no TCP half-close); fine for HTTP and WebSocket clients.
    let (mut read, mut write) = tcp.into_split();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        tokio::select! {
            biased;
            () = kick.notified() => return,
            changed = ended.changed() => if changed.is_err() { break },
            frame = stream.next() => match frame {
                Some(Ok(Message::Binary(data))) => {
                    let wrote = tokio::select! {
                        biased;
                        () = kick.notified() => return,
                        wrote = timeout(limit, write.write_all(&data)) => wrote,
                    };
                    if !matches!(wrote, Ok(Ok(()))) {
                        break;
                    }
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                _ => break, // close, error, or a text frame, which a tunnel never carries
            },
            n = read.read(&mut buf) => match n {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let sent = tokio::select! {
                        biased;
                        () = kick.notified() => return,
                        sent = timeout(limit, sink.send(Message::binary(buf[..n].to_vec()))) => sent,
                    };
                    if !matches!(sent, Ok(Ok(()))) {
                        return;
                    }
                }
            },
        }
    }
    let _ = timeout(limit, sink.close()).await;
}
