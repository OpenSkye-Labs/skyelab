//! This crate's test helpers on top of the shared harness (`skyelab_daemon::testing`: fake tailnet, test CA): a
//! counting backend and a blocking WebSocket client over real loopback sockets.
#![allow(dead_code)] // shared helpers; each test binary uses a subset

use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

pub use skyelab_daemon::testing::FakeTailnet;
use skyelab_daemon::testing::{NODE_NAME, TlsStream, ca, config, tls_connect};
use skyelab_daemon::{Backend, Config, Daemon, NotWired, StoreBackend};
use skyelab_protocol::{
    Call, ErrorCode, Project, Request, RpcError, ServerMessage, Thread, ThreadEvent, VERSION,
    Version,
};
use skyelab_store::Store;
use tungstenite::{Message, WebSocket};

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

pub fn test_config() -> Config {
    config(0)
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
    send(
        ws,
        1,
        Call::Hello {
            protocol,
            capabilities: vec![],
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
