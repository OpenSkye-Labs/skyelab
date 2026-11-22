//! The environment daemon: a WebSocket RPC server that only this user's own tailnet devices can reach.
//!
//! Tokio lives here only, on the daemon's own thread. The app talks to it through [`Daemon`], which is plain
//! sync code, so GPUI never needs tokio.
//!
//! Security gate, in order, for every TCP connection: the listener is bound to this node's tailnet IPs only;
//! `whois(peer_addr)` must say it is one of the user's own untagged devices ([`is_own_device`]); TLS with this
//! node's `tailscale cert` certificate (there is no plaintext mode); the WebSocket handshake must not come from a
//! browser (no `Origin`); the first request must be a compatible `hello`. Any error, including Tailscale being
//! down or slow, rejects the connection. Without a certificate the daemon doesn't start.

mod backend;
mod peers;
mod serve;
#[cfg(feature = "test-support")]
pub mod testing;
mod tls;

use std::net::{SocketAddr, TcpListener as StdListener};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use skyelab_protocol::ThreadEvent;
use skyelab_tailnet::{Tailnet, WhoIs, is_own_device};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore, broadcast, oneshot};
use tokio::time::timeout;
use tokio_rustls::server::TlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::handshake::server::{
    ErrorResponse, Request as HttpRequest, Response,
};
use tokio_tungstenite::tungstenite::http::{StatusCode, header};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

pub use backend::{Agents, Backend, MAX_EVENTS_PER_REPLY, NotWired, StoreBackend, event};
pub use peers::Peer;
use peers::{Peers, Registered, lock_peers};
use serve::serve;
pub use skyelab_tailnet::ADMIN_DNS_URL;
pub use tls::CertInfo;
use tls::Tls;

type Ws = WebSocketStream<TlsStream<TcpStream>>;

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
    /// Bound on whois plus the TLS and WebSocket handshakes. Past it the connection is rejected.
    pub gate_timeout: Duration,
    /// Connections being gated at once; more are dropped, so a flood can't spawn unbounded whois calls.
    pub max_pending: usize,
    /// How often `tailscale cert` runs again; it renews ahead of expiry, and the new cert is served at once.
    pub cert_check_every: Duration,
    /// After a failed renewal, the next try comes this soon (the current cert keeps being served).
    pub cert_retry: Duration,
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
            cert_check_every: Duration::from_secs(24 * 3600),
            cert_retry: Duration::from_secs(3600),
        }
    }
}

/// The running server. Dropping it stops the server and closes every connection.
pub struct Daemon {
    addrs: Vec<SocketAddr>,
    events: broadcast::Sender<ThreadEvent>,
    peers: Peers,
    tls: Arc<Tls>,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

struct Shared {
    config: Config,
    tailnet: SharedTailnet,
    backend: Arc<dyn Backend>,
    events: broadcast::Sender<ThreadEvent>,
    pending: Arc<Semaphore>,
    peers: Peers,
    next_peer: AtomicU64,
    tls: Arc<Tls>,
}

impl Daemon {
    /// Binds to this node's tailnet IPs and serves TLS on a new thread. Fails if Tailscale isn't up, or if it can't
    /// issue this node a certificate (MagicDNS or HTTPS certificates off): never a plaintext fallback.
    /// Blocks while `tailscale cert` runs, which can take tens of seconds on a first issuance.
    pub fn start(
        config: Config,
        tailnet: SharedTailnet,
        backend: Arc<dyn Backend>,
    ) -> Result<Self> {
        if config.max_in_flight == 0 || config.max_pending == 0 || config.max_message_bytes == 0 {
            bail!("daemon limits must be at least 1: {config:?}");
        }
        if config.cert_check_every.is_zero() || config.cert_retry.is_zero() {
            bail!("certificate check intervals must be above zero: {config:?}");
        }
        let me = tailnet
            .self_node()
            .context("can't start the daemon without a running tailnet")?;
        if me.tailscale_ips.is_empty() {
            bail!("this node has no tailnet IP to listen on");
        }
        let name = tls::node_name(&me.dns_name)?;
        let cert = tailnet
            .cert(&name)
            .with_context(|| format!("can't get a TLS certificate for {name}"))?;
        let tls = Arc::new(
            Tls::new(&cert, &name)
                .with_context(|| format!("can't use the TLS certificate for {name}"))?,
        );
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
        let peers = Peers::default();
        let shared = Arc::new(Shared {
            peers: peers.clone(),
            next_peer: AtomicU64::new(1),
            pending: Arc::new(Semaphore::new(config.max_pending)),
            tls: tls.clone(),
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
                    tokio::spawn(renew_loop(shared));
                    let _ = stop.await;
                });
                // Blocking whois calls can't be cancelled; their CLI timeout bounds them.
                runtime.shutdown_timeout(Duration::from_secs(1));
            })
            .context("spawn the daemon thread")?;
        Ok(Self {
            addrs,
            events,
            peers,
            tls,
            shutdown: Some(shutdown),
            thread: Some(thread),
        })
    }

    pub fn local_addrs(&self) -> &[SocketAddr] {
        &self.addrs
    }

    /// The certificate being served now (it changes when renewed).
    pub fn certificate(&self) -> CertInfo {
        self.tls.info()
    }

    /// Connections that passed the gate and are still open, oldest first.
    pub fn peers(&self) -> Vec<Peer> {
        lock_peers(&self.peers)
            .values()
            .map(|(p, _)| p.clone())
            .collect()
    }

    /// Closes the connection `id`. False if it's already gone.
    pub fn disconnect(&self, id: u64) -> bool {
        match lock_peers(&self.peers).get(&id) {
            Some((_, kick)) => {
                // notify_one keeps the wakeup if the serve loop isn't waiting right now.
                kick.notify_one();
                true
            }
            None => false,
        }
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

/// Re-runs `tailscale cert` on a timer and serves the result if it passes the start-time checks.
async fn renew_loop(shared: Arc<Shared>) {
    let config = &shared.config;
    // Tokio's clock stops while the machine sleeps, so "daily" is decided by the wall clock, checked at least
    // every `cert_retry`; otherwise a mostly-asleep laptop could go weeks without a check and serve an expired cert.
    let tick = config.cert_check_every.min(config.cert_retry);
    let mut due = SystemTime::now() + config.cert_check_every;
    loop {
        tokio::time::sleep(tick).await;
        if SystemTime::now() < due {
            continue;
        }
        let (tailnet, name) = (shared.tailnet.clone(), shared.tls.info().name);
        // The CLI call blocks; keep it off the runtime's workers so the gate keeps running.
        let fetched = tokio::task::spawn_blocking(move || tailnet.cert(&name)).await;
        let renewed = match fetched {
            Ok(Ok(cert)) => shared.tls.renew(&cert),
            Ok(Err(e)) => Err(e.into()),
            Err(e) => Err(e.into()),
        };
        due = SystemTime::now()
            + match renewed {
                Ok(()) => config.cert_check_every,
                Err(e) => {
                    eprintln!(
                        "skyelab-daemon: certificate renewal failed, still serving the current one: {e:#}"
                    );
                    config.cert_retry
                }
            };
    }
}

async fn handle(
    stream: TcpStream,
    peer: SocketAddr,
    shared: Arc<Shared>,
    permit: OwnedSemaphorePermit,
) {
    let (ws, who) = match timeout(
        shared.config.gate_timeout,
        admit(stream, peer, &shared, permit),
    )
    .await
    {
        Ok(Ok(admitted)) => admitted,
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
    let id = shared.next_peer.fetch_add(1, Ordering::Relaxed);
    let kick = Arc::new(Notify::new());
    let info = Peer {
        id,
        addr: peer,
        device: who.node.name.trim_end_matches('.').to_owned(),
        user: who.user_profile.login_name,
    };
    lock_peers(&shared.peers).insert(id, (info, kick.clone()));
    let _registered = Registered {
        peers: &shared.peers,
        id,
    };
    serve(ws, peer, &shared, &kick).await;
}

/// The gate. Nothing is read from the socket, not even a TLS ClientHello, until whois has passed.
async fn admit(
    stream: TcpStream,
    peer: SocketAddr,
    shared: &Shared,
    permit: OwnedSemaphorePermit,
) -> Result<(Ws, WhoIs), String> {
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
    let stream = shared
        .tls
        .acceptor
        .accept(stream)
        .await
        .map_err(|e| format!("TLS handshake from {:?} failed: {e}", checked.node.name))?;
    let ws =
        tokio_tungstenite::accept_hdr_async_with_config(stream, reject_browsers, Some(ws_config))
            .await
            .map_err(|e| format!("handshake from {:?} failed: {e}", checked.node.name))?;
    eprintln!(
        "skyelab-daemon: accepted {peer}: {:?} ({})",
        checked.node.name, checked.user_profile.login_name
    );
    Ok((ws, checked))
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
