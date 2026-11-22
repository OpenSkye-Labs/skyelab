//! Connecting: resolve, refuse non-tailnet addresses, dial, TLS, WebSocket handshake, hello. A handshake the
//! server hangs up on before sending a byte is a refusal by its gate.

use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use skyelab_protocol::{Call, ErrorCode, Hello, Reply, Request, ServerMessage, Version};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use super::{End, Io, Ws, lost, lost_as};
use crate::{DisconnectKind, Error, Options, guard};

/// Resolve, refuse non-tailnet addresses, dial the checked addresses, TLS, handshake, hello.
pub(super) struct Target {
    pub(super) host: String,
    pub(super) options: Options,
    pub(super) tls: Arc<ClientConfig>,
    pub(super) allow_loopback: bool,
    pub(super) plaintext: bool,
    pub(super) resolve_to: Option<std::net::IpAddr>,
    pub(super) protocol: Version,
}

pub(super) async fn connect(cx: Target) -> Result<(Ws, Hello), End> {
    let port = cx.options.port;
    let addrs = match cx.resolve_to {
        Some(ip) => guard::allowed(&cx.host, [SocketAddr::new(ip, port)], cx.allow_loopback),
        None => guard::resolve(&cx.host, port, cx.allow_loopback).await,
    }
    .map_err(lost)?;
    // Each address gets a share of the budget, so a black-holed first one can't starve the rest.
    let per_addr = cx.options.connect_timeout / addrs.len().max(1) as u32;
    let stream = dial(&addrs, per_addr).await.map_err(lost)?;
    // Who is really at that address, from this machine's own Tailscale, before a byte goes to it.
    let verified = match &cx.options.verify_peer {
        None => None,
        Some(verifier) => {
            let peer = stream
                .peer_addr()
                .map_err(|e| lost(format!("can't read the peer address: {e}")))?;
            let verify = verifier.clone();
            let id = tokio::task::spawn_blocking(move || (verify.0)(peer))
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r)
                .map_err(|why| lost_as(DisconnectKind::NotYourDevice, why))?;
            Some(id)
        }
    };
    let heard = Arc::new(AtomicBool::new(false));
    let stream = Heard {
        inner: stream,
        heard: heard.clone(),
    };
    // A handshake that fails before the server sent a byte: it hung up on us without a word, which is what the
    // daemon's gate does to a device it doesn't admit.
    let silent = || {
        if heard.load(Ordering::Relaxed) {
            DisconnectKind::Other
        } else {
            DisconnectKind::Refused
        }
    };
    let limit = cx.options.max_message_bytes;
    let config = WebSocketConfig::default()
        .max_message_size(Some(limit))
        .max_frame_size(Some(limit));
    let host = &cx.host;
    let (stream, scheme): (Box<dyn Io>, _) = if cx.plaintext {
        (Box::new(stream), "ws")
    } else {
        // Connect checked the host is a DNS name, so this can't fail; refuse rather than skip TLS if it ever does.
        let name = ServerName::try_from(host.clone())
            .map_err(|e| lost(format!("{host} isn't a DNS name: {e}")))?;
        let tls = tokio_rustls::TlsConnector::from(cx.tls.clone())
            .connect(name, stream)
            .await
            .map_err(|e| {
                let kind = if is_certificate_error(&e) {
                    DisconnectKind::Certificate
                } else {
                    silent()
                };
                lost_as(kind, format!("TLS with {host} failed: {e}"))
            })?;
        (Box::new(tls), "wss")
    };
    // The URL only names the Host header; the socket is already connected to a checked address.
    let (mut ws, _) = tokio_tungstenite::client_async_with_config(
        format!("{scheme}://{host}:{port}/"),
        stream,
        Some(config),
    )
    .await
    .map_err(|e| lost_as(silent(), format!("WebSocket handshake failed: {e}")))?;

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
            }) if h.protocol.major == cx.protocol.major => checked(h, verified).map(|h| (ws, h)),
            Ok(ServerMessage::Response {
                id: 0,
                result: Reply::Hello(h),
            }) => Err(End::Fatal(Error::UnsupportedVersion(format!(
                "the environment answered with protocol {}.{}",
                h.protocol.major, h.protocol.minor
            )))),
            Ok(ServerMessage::Error { id: Some(0), error })
                if error.code == ErrorCode::UnsupportedVersion =>
            {
                Err(End::Fatal(Error::UnsupportedVersion(error.message)))
            }
            Ok(other) => Err(lost(format!("unexpected answer to hello: {other:?}"))),
            Err(e) => Err(lost(format!("malformed answer to hello: {e}"))),
        };
    }
}

/// With a verified id, the hello must name the same node (or none), and the caller sees the verified one.
fn checked(mut hello: Hello, verified: Option<String>) -> Result<Hello, End> {
    let Some(verified) = verified else {
        return Ok(hello);
    };
    match hello.node_id.as_deref() {
        Some(said) if said != verified => Err(lost_as(
            DisconnectKind::IdentityMismatch,
            format!("it says it is node {said}, your Tailscale says {verified}"),
        )),
        _ => {
            hello.node_id = Some(verified);
            Ok(hello)
        }
    }
}

/// rustls reports a certificate it rejected as `InvalidData` wrapping `InvalidCertificate`.
fn is_certificate_error(e: &std::io::Error) -> bool {
    e.get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
        .is_some_and(|e| matches!(e, rustls::Error::InvalidCertificate(_)))
}

/// The TCP stream, noting whether the server ever sent a byte.
struct Heard<S> {
    inner: S,
    heard: Arc<AtomicBool>,
}

impl<S: AsyncRead + Unpin> AsyncRead for Heard<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        if buf.filled().len() > before {
            self.heard.store(true, Ordering::Relaxed);
        }
        poll
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Heard<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

async fn dial(addrs: &[SocketAddr], per_addr: Duration) -> Result<TcpStream, String> {
    let mut errors = Vec::new();
    for addr in addrs {
        match timeout(per_addr, TcpStream::connect(addr)).await {
            Ok(Ok(stream)) => {
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            Ok(Err(e)) => errors.push(format!("{addr}: {e}")),
            Err(_) => errors.push(format!("{addr}: timed out")),
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
