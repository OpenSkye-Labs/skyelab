//! The client side of SkyeLab's remote protocol: a WebSocket connection to a `skyelab-daemon` on the user's tailnet.
//!
//! Tokio runs on the client's own thread. The public API is sync: each RPC returns a [`Pending`] (block with
//! [`Pending::recv`], or await [`Pending::recv_async`] on any executor, GPUI's included), and connection state and
//! pushed events arrive on [`Client::updates`]. The client reconnects by itself with backoff and keeps the
//! connection alive with pings.
//!
//! Server trust, both required: the host (a MagicDNS name) is resolved first and only addresses inside Tailscale's
//! ranges are dialed ([`is_tailnet_ip`]), so a hostile DNS (the VPN off) can't point the client at a public or LAN
//! server; then the connection is `wss://` only, and the server's certificate must chain to the Mozilla roots and
//! name exactly the host dialled. The second check is what stops another carrier-grade-NAT subscriber, whose 100.x
//! address passes the first. There is no plaintext mode outside `cargo test`.
//!
//! Events: only watched threads ([`Client::watch`]) are delivered, each event exactly once and in seq order. A gap
//! in the pushed stream, a reconnect, or a server that dropped us for lagging is filled from the store with
//! `thread_events_since`, page by page.

mod conn;
mod guard;
#[cfg(test)]
mod tests;
mod tls;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

pub use guard::is_tailnet_ip;
use skyelab_protocol::{Call, Reply};
pub use skyelab_protocol::{
    ErrorCode, Hello, Project, RpcError, Thread, ThreadEvent, Version, is_magicdns_name,
};

#[derive(Debug, Clone)]
pub struct Options {
    pub port: u16,
    /// Largest message or frame the server may send; a bigger one closes the connection.
    pub max_message_bytes: usize,
    /// A ping goes out this often. A server silent for two intervals is treated as gone.
    pub ping_interval: Duration,
    /// Bound on resolve, TCP connect, TLS and WebSocket handshakes and hello together.
    pub connect_timeout: Duration,
    /// A request without a reply after this long fails with [`Error::Timeout`] (checked on each ping, so up to one
    /// `ping_interval` later).
    pub request_timeout: Duration,
    /// Reconnect delay: starts at `backoff_min`, doubles per failed attempt, capped at `backoff_max`.
    pub backoff_min: Duration,
    pub backoff_max: Duration,
    /// Checks each dialled address with this machine's own Tailscale before TLS starts. `None` (mobile, where no
    /// local Tailscale CLI exists) trusts the node id the server sends in hello, which a machine that took over the
    /// name can fake.
    pub verify_peer: Option<PeerVerifier>,
    /// Tests only: also dial loopback addresses. The field doesn't exist outside tests (`test-support`).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) allow_loopback: bool,
    /// Tests only: the protocol version sent in hello.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) protocol: Version,
    /// Tests only: trust these roots instead of the Mozilla ones.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) roots: Option<Arc<rustls::RootCertStore>>,
    /// Tests only: speak `ws://` (for fake servers that don't do TLS).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) plaintext: bool,
    /// Tests only: dial this address instead of resolving the host (the name is still checked by TLS).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) resolve_to: Option<std::net::IpAddr>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            port: skyelab_protocol::DEFAULT_PORT,
            max_message_bytes: 16 << 20,
            // The daemon drops a connection idle for 5 minutes; any frame, a ping included, resets that.
            ping_interval: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(30),
            backoff_min: Duration::from_millis(250),
            backoff_max: Duration::from_secs(30),
            verify_peer: None,
            #[cfg(any(test, feature = "test-support"))]
            allow_loopback: false,
            #[cfg(any(test, feature = "test-support"))]
            protocol: skyelab_protocol::VERSION,
            #[cfg(any(test, feature = "test-support"))]
            roots: None,
            #[cfg(any(test, feature = "test-support"))]
            plaintext: false,
            #[cfg(any(test, feature = "test-support"))]
            resolve_to: None,
        }
    }
}

/// Says who a dialled address is: `Ok` with the node StableID of one of the user's own devices, `Err` with why not.
/// Runs on a blocking thread, so it may shell out; it must return within its own timeout (an error fails closed).
///
/// With a verifier the hello's node id must equal the verified one, and [`Update::Connected`] carries the verified
/// id in `node_id`, so whatever the caller pins came from its own Tailscale.
#[derive(Clone)]
pub struct PeerVerifier(Arc<dyn Fn(std::net::SocketAddr) -> Result<String, String> + Send + Sync>);

impl PeerVerifier {
    pub fn new(
        verify: impl Fn(std::net::SocketAddr) -> Result<String, String> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(verify))
    }
}

impl std::fmt::Debug for PeerVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PeerVerifier")
    }
}

#[cfg(feature = "test-support")]
impl Options {
    /// Other crates' tests: also dial loopback, where their in-process daemon listens. Never in a real build.
    pub fn allow_loopback(mut self) -> Self {
        self.allow_loopback = true;
        self
    }

    /// Other crates' tests: trust `roots` (a test CA) instead of the Mozilla roots.
    pub fn trust_roots(mut self, roots: rustls::RootCertStore) -> Self {
        self.roots = Some(Arc::new(roots));
        self
    }

    /// Other crates' tests: dial `ip` instead of resolving the name. TLS still checks the name.
    pub fn resolve_to(mut self, ip: std::net::IpAddr) -> Self {
        self.resolve_to = Some(ip);
        self
    }

    /// Other crates' tests: speak `ws://` to a fake server that doesn't do TLS.
    pub fn plaintext(mut self) -> Self {
        self.plaintext = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("not connected to the environment")]
    NotConnected,
    /// The connection dropped before the reply. The request is never re-sent: it may have run.
    #[error("the connection dropped before the reply")]
    ConnectionLost,
    #[error("no reply in time")]
    Timeout,
    #[error("{message} ({code:?})", code = .0.code, message = .0.message)]
    Rpc(RpcError),
    /// Fatal: the client stops and doesn't reconnect.
    #[error("the environment speaks another protocol version: {0}")]
    UnsupportedVersion(String),
    #[error("the environment sent a reply of the wrong kind")]
    UnexpectedReply,
    #[error("the client has stopped")]
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// A connection passed hello. Watched threads are being resynced.
    Connected(Hello),
    /// The connection is gone (or never came up), with why. The client retries after a backoff.
    Disconnected(Disconnect),
    Event(ThreadEvent),
    /// The client stopped for good.
    Fatal(Error),
}

/// Why a connection went away or never came up. Match on `kind`; `message` is for people (and is `Display`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disconnect {
    pub kind: DisconnectKind,
    pub message: String,
}

impl std::fmt::Display for Disconnect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Add a `_` arm when matching: new kinds may come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DisconnectKind {
    /// TCP connected, then the server hung up before sending a single byte: its gate (whois) turned this device
    /// away. A daemon that is too busy to check us, or whose whois call failed, hangs up the same way.
    Refused,
    /// The server's certificate didn't verify (chain, expiry, or not exactly the dialled name).
    Certificate,
    /// The [`Options::verify_peer`] check refused the dialled address: this machine's Tailscale says it isn't one of
    /// the user's own untagged devices, or couldn't say (whois failed or timed out). Nothing was sent to it.
    NotYourDevice,
    /// The hello named another node than this machine's Tailscale reports for the dialled address.
    IdentityMismatch,
    /// Anything else: resolving, dialling, timeouts, a dropped connection, a malformed reply.
    Other,
}

/// One [`Client::watch`]. Pass it to [`Client::unwatch`] to end that watch, and only that one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[must_use = "unwatch with this id, or the thread stays watched"]
pub struct WatchId {
    pub thread_id: i64,
    n: u64,
}

/// The reply to one request.
#[must_use = "a request's result arrives here"]
pub struct Pending<T>(flume::Receiver<Result<T, Error>>);

impl<T> Pending<T> {
    pub fn recv(self) -> Result<T, Error> {
        self.0.recv().unwrap_or(Err(Error::Stopped))
    }

    pub fn recv_timeout(self, timeout: Duration) -> Result<T, Error> {
        match self.0.recv_timeout(timeout) {
            Ok(result) => result,
            Err(flume::RecvTimeoutError::Timeout) => Err(Error::Timeout),
            Err(flume::RecvTimeoutError::Disconnected) => Err(Error::Stopped),
        }
    }

    pub async fn recv_async(self) -> Result<T, Error> {
        self.0.recv_async().await.unwrap_or(Err(Error::Stopped))
    }
}

/// A connection to one environment. Dropping it closes the connection; the thread exits on its own.
///
/// A request made while disconnected fails with [`Error::NotConnected`] (one made just as a connection comes up
/// may go out on it instead).
pub struct Client {
    commands: flume::Sender<conn::Command>,
    updates: flume::Receiver<Update>,
    hello: Arc<Mutex<Option<Hello>>>,
    next_watch: AtomicU64,
}

impl Client {
    /// Starts connecting to `host`, the environment's MagicDNS name (`machine.tailnet.ts.net`), in the background.
    /// Never blocks on the network. An IP address is refused: a certificate can't vouch for one.
    pub fn connect(host: impl Into<String>, options: Options) -> std::io::Result<Self> {
        let invalid = |msg: String| std::io::Error::new(std::io::ErrorKind::InvalidInput, msg);
        if options.ping_interval.is_zero() || options.backoff_min.is_zero() {
            return Err(invalid(
                "ping_interval and backoff_min must be above zero".into(),
            ));
        }
        // MagicDNS names are lowercase; one trailing dot names the same host.
        let host = host.into();
        let host = host.strip_suffix('.').unwrap_or(&host).to_ascii_lowercase();
        let is_dns_name = !host.ends_with('.')
            && matches!(
                rustls::pki_types::ServerName::try_from(host.as_str()),
                Ok(rustls::pki_types::ServerName::DnsName(_))
            );
        if !is_dns_name {
            return Err(invalid(format!(
                "{host:?} isn't a MagicDNS name; connect by name (machine.tailnet.ts.net) so TLS can check the server"
            )));
        }
        #[cfg(any(test, feature = "test-support"))]
        let roots = options.roots.clone().unwrap_or_else(tls::webpki_roots);
        #[cfg(not(any(test, feature = "test-support")))]
        let roots = tls::webpki_roots();
        let tls = tls::config(roots)?;
        let (commands, commands_rx) = flume::unbounded();
        let (updates_tx, updates) = flume::unbounded();
        let hello = Arc::new(Mutex::new(None));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let shared_hello = hello.clone();
        std::thread::Builder::new()
            .name("skyelab-client".into())
            .spawn(move || {
                runtime.block_on(conn::run(
                    host,
                    options,
                    tls,
                    commands_rx,
                    updates_tx,
                    shared_hello,
                ));
                // A DNS lookup in flight can't be cancelled; don't wait for it.
                runtime.shutdown_background();
            })?;
        Ok(Self {
            commands,
            updates,
            hello,
            next_watch: AtomicU64::new(1),
        })
    }

    /// Connection changes and watched threads' events, in order.
    pub fn updates(&self) -> &flume::Receiver<Update> {
        &self.updates
    }

    /// The server's hello on the current connection, `None` while disconnected.
    pub fn hello(&self) -> Option<Hello> {
        self.hello
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn list_projects(&self) -> Pending<Vec<Project>> {
        self.call(Call::ListProjects, |r| match r {
            Reply::Projects(p) => Some(p),
            _ => None,
        })
    }

    pub fn list_threads(&self, project_id: i64) -> Pending<Vec<Thread>> {
        self.call(Call::ListThreads { project_id }, |r| match r {
            Reply::Threads(t) => Some(t),
            _ => None,
        })
    }

    /// One page of events after `seq`. To follow a thread, use [`Client::watch`], which pages and dedupes for you.
    pub fn thread_events_since(&self, thread_id: i64, seq: i64) -> Pending<Vec<ThreadEvent>> {
        self.call(Call::ThreadEventsSince { thread_id, seq }, |r| match r {
            Reply::Events(e) => Some(e),
            _ => None,
        })
    }

    /// Returns the new thread's id.
    pub fn start_thread(&self, project_id: i64, prompt: impl Into<String>) -> Pending<i64> {
        let prompt = prompt.into();
        self.call(Call::StartThread { project_id, prompt }, |r| match r {
            Reply::ThreadStarted { thread_id } => Some(thread_id),
            _ => None,
        })
    }

    pub fn send_message(&self, thread_id: i64, text: impl Into<String>) -> Pending<()> {
        let text = text.into();
        self.call(Call::SendMessage { thread_id, text }, ok)
    }

    pub fn interrupt(&self, thread_id: i64) -> Pending<()> {
        self.call(Call::Interrupt { thread_id }, ok)
    }

    /// Delivers the thread's events after `after_seq` on [`Client::updates`], then every new one, each once, in
    /// order, across reconnects. Watching a thread again (a reopened tab) replays from the new `after_seq`, so a
    /// watcher already following it sees those events again and must drop them by seq.
    ///
    /// The thread stays watched while any of its watches is live: a late [`Client::unwatch`] of an old watch
    /// never ends a newer one.
    pub fn watch(&self, thread_id: i64, after_seq: i64) -> WatchId {
        let id = WatchId {
            thread_id,
            n: self.next_watch.fetch_add(1, Ordering::Relaxed),
        };
        self.command(conn::Command::Watch {
            id,
            after_seq: after_seq.max(0), // seqs start at 1
        });
        id
    }

    /// Ends one watch. Unwatching the same id twice does nothing.
    pub fn unwatch(&self, id: WatchId) {
        self.command(conn::Command::Unwatch(id));
    }

    fn call<T: Send + 'static>(
        &self,
        call: Call,
        pick: impl FnOnce(Reply) -> Option<T> + Send + 'static,
    ) -> Pending<T> {
        let (tx, rx) = flume::bounded(1);
        let reply: conn::ReplyFn = Box::new(move |result| {
            let result = result.and_then(|r| pick(r).ok_or(Error::UnexpectedReply));
            let _ = tx.send(result); // the caller may have dropped its Pending
        });
        self.command(conn::Command::Call { call, reply });
        Pending(rx)
    }

    fn command(&self, command: conn::Command) {
        // The connection task has stopped (a fatal error): fail the request right away.
        if let Err(flume::SendError(conn::Command::Call { reply, .. })) =
            self.commands.send(command)
        {
            reply(Err(Error::Stopped));
        }
    }
}

fn ok(reply: Reply) -> Option<()> {
    matches!(reply, Reply::Ok).then_some(())
}
