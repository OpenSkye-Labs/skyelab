//! How a [`crate::Client`] connects: [`Options`] and the [`PeerVerifier`] it may carry.

use std::sync::Arc;
use std::time::Duration;

#[cfg(any(test, feature = "test-support"))]
use skyelab_protocol::Version;

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
    /// This install's device key, sent in hello; the machine dialled admits only keys approved there. `None`
    /// can never be approved. Load it with [`load_or_create_device_key`] (or the keychain on macOS).
    pub device_key: Option<String>,
    /// Checks each dialled address with this machine's own Tailscale before TLS starts. `None` (mobile, where no
    /// local Tailscale CLI exists) trusts the node id the server sends in hello, which a machine that took over the
    /// name can fake.
    pub verify_peer: Option<PeerVerifier>,
    /// Hold every connection after hello until [`Client::approve`] approves its hello: until then no request or
    /// watch refill is sent (requests fail with [`Error::NotConnected`]; only keepalive pings go out) and no pushed
    /// event is delivered.
    /// For a caller that checks the server's identity (a pin) before trusting a connection, reconnects included.
    pub approve_each_connection: bool,
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
            device_key: None,
            verify_peer: None,
            approve_each_connection: false,
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

impl Options {
    /// The defaults with this install's device key. A struct literal can't build `Options` where `test-support`
    /// adds its private fields, and assigning after `default()` trips clippy where it doesn't.
    pub fn with_device_key(mut self, key: String) -> Self {
        self.device_key = Some(key);
        self
    }

    /// The defaults with every connection checked by `verify` (same reason as [`Options::with_device_key`]).
    pub fn with_verify_peer(mut self, verify: PeerVerifier) -> Self {
        self.verify_peer = Some(verify);
        self
    }
}

/// Says who a dialled address is: `Ok` with the node StableID of one of the user's own devices, `Err` with why not.
/// Runs on a blocking thread, so it may shell out; it must return within its own timeout (an error fails closed).
///
/// With a verifier the hello's node id must equal the verified one, and [`Update::Connected`] carries the verified
/// id in `node_id`, so whatever the caller pins came from its own Tailscale.
#[derive(Clone)]
pub struct PeerVerifier(
    pub(crate) Arc<dyn Fn(std::net::SocketAddr) -> Result<String, String> + Send + Sync>,
);

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
