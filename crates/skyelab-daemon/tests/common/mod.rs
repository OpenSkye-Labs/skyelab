//! Shared harness: a fake Tailnet (with a test CA issuing this node's cert), a counting backend, and a blocking
//! TLS client over real loopback sockets.
#![allow(dead_code)] // shared helpers; each test binary uses a subset

use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use skyelab_daemon::testing::{NODE_NAME, TlsStream, ca, tls_connect};
use skyelab_daemon::{Backend, Config, Daemon, NotWired, StoreBackend};
use skyelab_protocol::{
    Call, ErrorCode, Project, Request, RpcError, ServerMessage, Thread, ThreadEvent, VERSION,
    Version,
};
use skyelab_store::Store;
use skyelab_tailnet::{Cert, Error, Node, Tailnet, UserProfile, WhoIs, WhoIsNode};
use tungstenite::{Message, WebSocket};

pub const ME: u64 = 1001;

/// The test client's device key; [`start`] approves it for the phone ("nPhone").
pub const KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Clone, Copy)]
pub enum Peer {
    Own,
    /// Another of the user's own untagged devices, with this StableID.
    OwnNode(&'static str),
    OtherUser,
    Tagged,
    WhoisError,
    Hang(Duration),
}

pub struct FakeTailnet {
    pub ips: Vec<IpAddr>,
    pub down: AtomicBool,
    pub peer: Mutex<Peer>,
    pub whois_calls: AtomicUsize,
    /// What `cert` returns; an `Err` is the CLI's error text.
    pub cert: Mutex<Result<Cert, String>>,
    /// `cert` sleeps this long first (a hung `tailscale cert`).
    pub cert_hang: Mutex<Duration>,
    pub cert_calls: AtomicUsize,
    /// `Self.DNSName`; empty means MagicDNS is off.
    pub dns_name: Mutex<String>,
}

impl Default for FakeTailnet {
    fn default() -> Self {
        Self {
            ips: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
            down: AtomicBool::new(false),
            peer: Mutex::new(Peer::Own),
            whois_calls: AtomicUsize::new(0),
            cert: Mutex::new(Ok(ca().issue(NODE_NAME))),
            cert_hang: Mutex::new(Duration::ZERO),
            cert_calls: AtomicUsize::new(0),
            dns_name: Mutex::new(format!("{NODE_NAME}.")),
        }
    }
}

impl FakeTailnet {
    pub fn new(peer: Peer) -> Arc<Self> {
        Arc::new(Self {
            peer: Mutex::new(peer),
            ..Self::default()
        })
    }
}

pub fn self_node(ips: Vec<IpAddr>) -> Node {
    Node {
        stable_id: "nSelf".into(),
        hostname: "desktop".into(),
        dns_name: "desktop.example.ts.net.".into(),
        tailscale_ips: ips,
        os: "macOS".into(),
        online: true,
        user_id: Some(ME),
        tags: vec![],
    }
}

fn whois(user: u64, tags: Vec<String>) -> WhoIs {
    whois_node("nPhone", user, tags)
}

fn whois_node(stable_id: &str, user: u64, tags: Vec<String>) -> WhoIs {
    WhoIs {
        node: WhoIsNode {
            stable_id: stable_id.into(),
            name: "phone.example.ts.net.".into(),
            user: Some(user),
            tags,
        },
        user_profile: UserProfile {
            id: Some(user),
            login_name: format!("user{user}@example.com"),
            display_name: "Someone".into(),
        },
    }
}

impl Tailnet for FakeTailnet {
    fn self_node(&self) -> Result<Node, Error> {
        if self.down.load(Ordering::SeqCst) {
            return Err(Error::NotRunning("Stopped".into()));
        }
        Ok(Node {
            dns_name: self.dns_name.lock().expect("test setup").clone(),
            ..self_node(self.ips.clone())
        })
    }

    fn peers(&self) -> Result<Vec<Node>, Error> {
        Ok(vec![])
    }

    fn whois(&self, _addr: SocketAddr) -> Result<WhoIs, Error> {
        self.whois_calls.fetch_add(1, Ordering::SeqCst);
        // Copy out first: holding the lock through a Hang would serialize every whois.
        let peer = *self.peer.lock().expect("test setup");
        match peer {
            Peer::Own => Ok(whois(ME, vec![])),
            Peer::OwnNode(id) => Ok(whois_node(id, ME, vec![])),
            Peer::OtherUser => Ok(whois(2002, vec![])),
            Peer::Tagged => Ok(whois(ME, vec!["tag:server".into()])),
            Peer::WhoisError => Err(Error::Command {
                command: "whois",
                stderr: "no such peer".into(),
            }),
            Peer::Hang(d) => {
                std::thread::sleep(d);
                Ok(whois(ME, vec![]))
            }
        }
    }

    fn cert(&self, _name: &str) -> Result<Cert, Error> {
        self.cert_calls.fetch_add(1, Ordering::SeqCst);
        let hang = *self.cert_hang.lock().expect("test setup");
        std::thread::sleep(hang);
        self.cert
            .lock()
            .expect("test setup")
            .clone()
            .map_err(|stderr| Error::Command {
                command: "cert",
                stderr,
            })
    }
}

/// A backend that only counts calls, so a test can prove nothing was served.
#[derive(Default)]
pub struct Counting {
    pub calls: AtomicUsize,
}

impl Backend for Counting {
    fn list_projects(&self) -> Result<Vec<Project>, RpcError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(vec![])
    }
    fn list_threads(&self, _: i64) -> Result<Vec<Thread>, RpcError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(vec![])
    }
    fn thread_events_since(&self, _: i64, _: i64) -> Result<Vec<ThreadEvent>, RpcError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(vec![])
    }
    fn start_thread(&self, _: i64, _: &str) -> Result<i64, RpcError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(1)
    }
    fn send_message(&self, _: i64, _: &str) -> Result<(), RpcError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn interrupt(&self, _: i64) -> Result<(), RpcError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// Approves [`KEY`] on the phone, so tests about other things get past hello.
pub fn test_config() -> Config {
    Config {
        port: 0,
        gate_timeout: Duration::from_millis(500),
        approved: vec![skyelab_daemon::Approval {
            node_id: "nPhone".into(),
            key_hash: skyelab_protocol::device_key_hash(KEY),
        }],
        ..Config::default()
    }
}

pub fn start(tailnet: Arc<FakeTailnet>, backend: Arc<dyn Backend>, config: Config) -> Daemon {
    Daemon::start(config, tailnet, backend).expect("test setup")
}

pub fn store_backend(store: Store) -> Arc<dyn Backend> {
    Arc::new(StoreBackend::new(store, NotWired))
}

pub type Client = WebSocket<TlsStream>;

fn ws_error(
    e: tungstenite::HandshakeError<tungstenite::ClientHandshake<TlsStream>>,
) -> tungstenite::Error {
    match e {
        tungstenite::HandshakeError::Failure(e) => e,
        tungstenite::HandshakeError::Interrupted(_) => panic!("blocking handshake interrupted"),
    }
}

/// Connects over TLS (trusting only the harness CA, checking the name) and completes the WebSocket handshake,
/// or returns why it failed.
pub fn try_connect(addr: SocketAddr) -> Result<Client, tungstenite::Error> {
    let tls = tls_connect(addr, NODE_NAME, ca().root())?;
    tungstenite::client(format!("wss://{NODE_NAME}/"), tls)
        .map(|(ws, _)| ws)
        .map_err(ws_error)
}

pub fn try_connect_with_origin(
    addr: SocketAddr,
    origin: &str,
) -> Result<Client, tungstenite::Error> {
    use tungstenite::client::IntoClientRequest;
    let mut req = format!("wss://{NODE_NAME}/")
        .into_client_request()
        .expect("test setup");
    req.headers_mut()
        .insert("Origin", origin.parse().expect("test setup"));
    let tls = tls_connect(addr, NODE_NAME, ca().root())?;
    tungstenite::client(req, tls)
        .map(|(ws, _)| ws)
        .map_err(ws_error)
}

/// A plain TCP socket with a read timeout, for tests that speak raw bytes.
pub fn tcp(addr: SocketAddr) -> TcpStream {
    let tcp = TcpStream::connect(addr).expect("test setup");
    tcp.set_read_timeout(Some(Duration::from_secs(5)))
        .expect("test setup");
    tcp
}

pub fn connect(addr: SocketAddr) -> Client {
    try_connect(addr).expect("test setup")
}

pub fn send(ws: &mut Client, id: u64, call: Call) {
    ws.send(Message::text(
        serde_json::to_string(&Request { id, call }).expect("test setup"),
    ))
    .expect("test setup");
}

/// The next server message, skipping control frames. `None` once the server closed the connection.
/// Panics on a read timeout, so a server that goes silent can't pass for one that closed.
pub fn recv(ws: &mut Client) -> Option<ServerMessage> {
    loop {
        match ws.read() {
            Ok(Message::Text(t)) => return Some(serde_json::from_str(&t).expect("test setup")),
            Ok(_) => continue,
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                panic!("read timed out: the server neither answered nor closed")
            }
            Err(_) => return None,
        }
    }
}

pub fn hello_with(ws: &mut Client, protocol: Version) -> Option<ServerMessage> {
    hello_as(ws, protocol, Some(KEY))
}

/// Hello with this device key, or none.
pub fn hello_key(ws: &mut Client, key: Option<&str>) -> Option<ServerMessage> {
    hello_as(ws, VERSION, key)
}

fn hello_as(ws: &mut Client, protocol: Version, key: Option<&str>) -> Option<ServerMessage> {
    send(
        ws,
        1,
        Call::Hello {
            protocol,
            capabilities: vec![],
            device_key: key.map(str::to_owned),
        },
    );
    recv(ws)
}

/// Connects and says hello; panics unless the server answered with its hello.
pub fn connect_hello(addr: SocketAddr) -> Client {
    let mut ws = connect(addr);
    match hello_with(&mut ws, VERSION) {
        Some(ServerMessage::Response { id: 1, .. }) => ws,
        other => panic!("hello failed: {other:?}"),
    }
}

pub fn error_code(msg: Option<ServerMessage>) -> ErrorCode {
    match msg {
        Some(ServerMessage::Error { error, .. }) => error.code,
        other => panic!("expected an error, got {other:?}"),
    }
}
