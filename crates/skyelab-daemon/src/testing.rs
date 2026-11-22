//! The remote-test harness every crate shares (feature `test-support`): test certificates, a blocking TLS client,
//! a configurable fake tailnet, and an [`Env`] that serves a store through an in-process daemon on loopback.
//! Never enabled in a shipped build.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rcgen::{
    BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair, KeyUsagePurpose,
    date_time_ymd,
};
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use skyelab_protocol::{ErrorCode, Project, RpcError, Thread, ThreadEvent};
use skyelab_store::{NewThread, Store};
use skyelab_tailnet::{Cert, Error as TailnetError, Node, Tailnet, UserProfile, WhoIs, WhoIsNode};

use crate::{Agents, Backend, Config, Daemon, StoreBackend};

/// The MagicDNS name the fake tailnets in tests give this node.
pub const NODE_NAME: &str = "desktop.example.ts.net";

/// One CA per test process, shared by every harness that needs this node's cert.
pub fn ca() -> &'static TestCa {
    static CA: OnceLock<TestCa> = OnceLock::new();
    CA.get_or_init(TestCa::new)
}

/// A throwaway certificate authority.
pub struct TestCa {
    issuer: Issuer<'static, KeyPair>,
    root: CertificateDer<'static>,
    root_pem: String,
}

impl Default for TestCa {
    fn default() -> Self {
        Self::new()
    }
}

impl TestCa {
    pub fn new() -> Self {
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("ca params");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        // A distinct name per CA, so a cert from another one is an unknown issuer, not a bad signature.
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        params
            .distinguished_name
            .push(DnType::CommonName, format!("SkyeLab Test CA {n}"));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let key = KeyPair::generate().expect("ca key");
        let root = params.self_signed(&key).expect("ca cert");
        Self {
            issuer: Issuer::new(params, key),
            root: root.der().clone(),
            root_pem: root.pem(),
        }
    }

    /// The root to trust.
    pub fn root(&self) -> CertificateDer<'static> {
        self.root.clone()
    }

    /// A leaf for `names`, valid from 2020 until `until` (year, month, day). PEM chain is leaf then CA.
    pub fn issue_until(&self, names: &[&str], until: (i32, u8, u8)) -> Cert {
        let names: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
        let mut params = CertificateParams::new(names).expect("leaf params");
        params.not_before = date_time_ymd(2020, 1, 1);
        params.not_after = date_time_ymd(until.0, until.1, until.2);
        let key = KeyPair::generate().expect("leaf key");
        let leaf = params.signed_by(&key, &self.issuer).expect("sign leaf");
        Cert {
            cert_pem: format!("{}{}", leaf.pem(), self.root_pem).into_bytes(),
            key_pem: key.serialize_pem().into_bytes(),
        }
    }

    /// A valid leaf for `name` (until 2099).
    pub fn issue(&self, name: &str) -> Cert {
        self.issue_until(&[name], (2099, 1, 1))
    }

    /// A leaf for `name` that expired in 2021.
    pub fn issue_expired(&self, name: &str) -> Cert {
        self.issue_until(&[name], (2021, 1, 1))
    }
}

pub type TlsStream = StreamOwned<ClientConnection, TcpStream>;

/// A blocking TLS connection to `addr` that trusts only `root` and checks the cert names `name`.
pub fn tls_connect(
    addr: SocketAddr,
    name: &str,
    root: CertificateDer<'static>,
) -> std::io::Result<TlsStream> {
    let mut roots = RootCertStore::empty();
    roots.add(root).expect("add test root");
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("tls 1.3")
        .with_root_certificates(roots)
        .with_no_client_auth();
    let server = ServerName::try_from(name.to_owned()).expect("test server name");
    let conn = ClientConnection::new(Arc::new(config), server).expect("client connection");
    let tcp = TcpStream::connect(addr)?;
    tcp.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut tls = StreamOwned::new(conn, tcp);
    // Finish the handshake here, so a refused certificate fails this call, not the first WebSocket read.
    while tls.conn.is_handshaking() {
        tls.conn.complete_io(&mut tls.sock)?;
    }
    Ok(tls)
}

/// The tailnet user every fake node belongs to unless a test says otherwise.
pub const ME: u64 = 1001;

/// Who `FakeTailnet::whois` says is calling.
#[derive(Clone, Copy, Debug)]
pub enum Peer {
    /// One of the user's own untagged devices: the gate admits it.
    Own,
    OtherUser,
    /// The user's node, but tagged: refused.
    Tagged,
    WhoisError,
    /// Admitted, after this long.
    Hang(Duration),
}

/// A tailnet node on loopback.
pub fn node(id: &str, dns: &str, user: u64, online: bool, tags: &[&str]) -> Node {
    Node {
        stable_id: id.into(),
        hostname: dns.split('.').next().unwrap_or_default().into(),
        dns_name: format!("{dns}."),
        tailscale_ips: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
        os: "macOS".into(),
        online,
        user_id: Some(user),
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
    }
}

/// The daemon's view of the tailnet: this node is `NODE_NAME` on `ips` (127.0.0.1), `peers` is what
/// `tailscale status` lists, and whois answers as `peer` says. The atomic and `Mutex` knobs can change while a
/// daemon uses them.
pub struct FakeTailnet {
    pub ips: Vec<IpAddr>,
    /// `self_node` fails like a stopped Tailscale.
    pub down: AtomicBool,
    pub peer: Mutex<Peer>,
    pub peers: Vec<Node>,
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
            peers: Vec::new(),
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

    pub fn set_peer(&self, peer: Peer) {
        *self.peer.lock().expect("peer lock") = peer;
    }

    pub fn whois_calls(&self) -> usize {
        self.whois_calls.load(Ordering::SeqCst)
    }
}

fn whois(user: u64, tags: Vec<String>) -> WhoIs {
    WhoIs {
        node: WhoIsNode {
            stable_id: "nPhone".into(),
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
    fn self_node(&self) -> Result<Node, TailnetError> {
        if self.down.load(Ordering::SeqCst) {
            return Err(TailnetError::NotRunning("Stopped".into()));
        }
        Ok(Node {
            tailscale_ips: self.ips.clone(),
            dns_name: self.dns_name.lock().expect("dns lock").clone(),
            ..node("nSelf", NODE_NAME, ME, true, &[])
        })
    }

    fn peers(&self) -> Result<Vec<Node>, TailnetError> {
        Ok(self.peers.clone())
    }

    fn whois(&self, _addr: SocketAddr) -> Result<WhoIs, TailnetError> {
        self.whois_calls.fetch_add(1, Ordering::SeqCst);
        // Copy out first: holding the lock through a Hang would serialize every whois.
        let peer = *self.peer.lock().expect("peer lock");
        match peer {
            Peer::Own => Ok(whois(ME, vec![])),
            Peer::OtherUser => Ok(whois(2002, vec![])),
            Peer::Tagged => Ok(whois(ME, vec!["tag:server".into()])),
            Peer::WhoisError => Err(TailnetError::Command {
                command: "whois",
                stderr: "no such peer".into(),
            }),
            Peer::Hang(d) => {
                std::thread::sleep(d);
                Ok(whois(ME, vec![]))
            }
        }
    }

    fn cert(&self, _name: &str) -> Result<Cert, TailnetError> {
        self.cert_calls.fetch_add(1, Ordering::SeqCst);
        let hang = *self.cert_hang.lock().expect("cert lock");
        std::thread::sleep(hang);
        self.cert
            .lock()
            .expect("cert lock")
            .clone()
            .map_err(|stderr| TailnetError::Command {
                command: "cert",
                stderr,
            })
    }
}

/// Daemon settings for tests: `port` (0 picks one) and a short gate timeout.
pub fn config(port: u16) -> Config {
    Config {
        port,
        gate_timeout: Duration::from_millis(500),
        ..Config::default()
    }
}

/// What the agents backend was asked to run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ran {
    Start(i64, String),
    Send(i64, String),
    Interrupt(i64),
}

/// The agent calls an [`Env`] saw. With `fail`, every call is still recorded, then fails like an unwired desktop.
#[derive(Default)]
pub struct Recorder {
    pub ran: Mutex<Vec<Ran>>,
    pub fail: AtomicBool,
}

/// Records calls; a start creates the thread row (account "work") with its prompt, so it can be listed and watched.
struct RecordingAgents {
    recorder: Arc<Recorder>,
    store: Arc<Mutex<Store>>,
    cwd: PathBuf,
}

impl RecordingAgents {
    fn record(&self, call: Ran) -> Result<(), RpcError> {
        self.recorder.ran.lock().expect("ran lock").push(call);
        if self.recorder.fail.load(Ordering::SeqCst) {
            return Err(RpcError::new(
                ErrorCode::NotWired,
                "running agents remotely is not wired up yet",
            ));
        }
        Ok(())
    }
}

impl Agents for RecordingAgents {
    fn start_thread(&self, project_id: i64, prompt: &str) -> Result<i64, RpcError> {
        self.record(Ran::Start(project_id, prompt.into()))?;
        let mut store = self.store.lock().expect("store lock");
        let thread = store
            .create_thread(&NewThread {
                project_id,
                title: prompt,
                cwd: &self.cwd,
                account_id: "work",
                status: "running",
            })
            .expect("thread");
        // The desktop's prompt row (`thread_model::Record::Prompt`): kind "prompt", the text as a JSON string.
        let payload = serde_json::Value::String(prompt.into()).to_string();
        store
            .append_event(thread.id, "prompt", &payload)
            .expect("append");
        Ok(thread.id)
    }
    fn send_message(&self, thread_id: i64, text: &str) -> Result<(), RpcError> {
        self.record(Ran::Send(thread_id, text.into()))
    }
    fn interrupt(&self, thread_id: i64) -> Result<(), RpcError> {
        self.record(Ran::Interrupt(thread_id))
    }
}

/// The store backend, except that the next `fail_lists` thread listings fail.
struct Flaky {
    inner: StoreBackend<RecordingAgents>,
    fail_lists: Arc<AtomicUsize>,
}

impl Backend for Flaky {
    fn list_projects(&self) -> Result<Vec<Project>, RpcError> {
        self.inner.list_projects()
    }
    fn list_threads(&self, project_id: i64) -> Result<Vec<Thread>, RpcError> {
        let failing = self
            .fail_lists
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
        if failing.is_ok() {
            return Err(RpcError::new(ErrorCode::Internal, "store busy"));
        }
        self.inner.list_threads(project_id)
    }
    fn thread_events_since(&self, thread_id: i64, seq: i64) -> Result<Vec<ThreadEvent>, RpcError> {
        self.inner.thread_events_since(thread_id, seq)
    }
    fn start_thread(&self, project_id: i64, prompt: &str) -> Result<i64, RpcError> {
        self.inner.start_thread(project_id, prompt)
    }
    fn send_message(&self, thread_id: i64, text: &str) -> Result<(), RpcError> {
        self.inner.send_message(thread_id, text)
    }
    fn interrupt(&self, thread_id: i64) -> Result<(), RpcError> {
        self.inner.interrupt(thread_id)
    }
}

/// Another machine: a store on disk (one project "app", one thread on account "work") and, once served, a daemon
/// on a fixed loopback port with the recording agents backend.
pub struct Env {
    /// The store file, for tests that open their own connection.
    pub db: PathBuf,
    pub store: Arc<Mutex<Store>>,
    pub project_id: i64,
    pub thread_id: i64,
    pub tailnet: Arc<FakeTailnet>,
    pub agents: Arc<Recorder>,
    /// How many of the next thread listings fail.
    pub fail_lists: Arc<AtomicUsize>,
    /// Where `serve` listens: picked by the first `serve`, then the same on every restart. 0 until then.
    pub port: u16,
    pub daemon: Option<Daemon>,
    cwd: PathBuf,
    // Last, so the daemon and store let go of the files before the directory goes.
    _dir: tempfile::TempDir,
}

impl Env {
    /// Project "app" at /src/app with thread "fix it", not served yet.
    pub fn new() -> Self {
        Self::with(FakeTailnet::default(), "/src/app", "fix it")
    }

    /// [`Env::new`], served.
    pub fn start() -> Self {
        let mut env = Self::new();
        env.serve();
        env
    }

    /// Project "app" at `cwd` with one thread titled `title`, on `tailnet`, not served yet.
    pub fn with(tailnet: FakeTailnet, cwd: &str, title: &str) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = dir.path().join("store.db");
        let store = Store::open(&db).expect("open store");
        let cwd = PathBuf::from(cwd);
        let project = store.create_project("app", &cwd).expect("project");
        let thread = store
            .create_thread(&NewThread {
                project_id: project.id,
                title,
                cwd: &cwd,
                account_id: "work",
                status: "idle",
            })
            .expect("thread");
        Self {
            _dir: dir,
            db,
            store: Arc::new(Mutex::new(store)),
            project_id: project.id,
            thread_id: thread.id,
            tailnet: Arc::new(tailnet),
            agents: Arc::default(),
            fail_lists: Arc::default(),
            port: 0,
            daemon: None,
            cwd,
        }
    }

    /// A backend over this env's store and recorder, for a test that starts its own daemon.
    pub fn backend(&self) -> Arc<dyn Backend> {
        Arc::new(Flaky {
            inner: StoreBackend::shared(
                self.store.clone(),
                RecordingAgents {
                    recorder: self.agents.clone(),
                    store: self.store.clone(),
                    cwd: self.cwd.clone(),
                },
            ),
            fail_lists: self.fail_lists.clone(),
        })
    }

    /// (Re)starts the daemon: on a free port the first time, then on that same port.
    pub fn serve(&mut self) {
        self.daemon = None;
        let backend = self.backend();
        let mut last = None;
        for _ in 0..100 {
            match Daemon::start(config(self.port), self.tailnet.clone(), backend.clone()) {
                Ok(d) => {
                    self.port = d.local_addrs()[0].port();
                    self.daemon = Some(d);
                    return;
                }
                // The old listener's port can take a moment to free.
                Err(e) => {
                    last = Some(e);
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
        panic!("daemon didn't start on port {}: {last:?}", self.port);
    }

    pub fn stop(&mut self) {
        self.daemon = None;
    }

    /// Stores an event on `thread_id` without pushing it.
    pub fn append(&self, thread_id: i64, kind: &str, payload: &str) -> ThreadEvent {
        let mut store = self.store.lock().expect("store lock");
        crate::event(
            store
                .append_event(thread_id, kind, payload)
                .expect("append"),
        )
    }

    /// Stores an event on the env's thread and pushes it to connected clients, as the desktop does.
    pub fn push(&self, kind: &str, payload: &str) -> ThreadEvent {
        let event = self.append(self.thread_id, kind, payload);
        if let Some(daemon) = &self.daemon {
            daemon.push_event(event.clone());
        }
        event
    }

    pub fn ran(&self) -> Vec<Ran> {
        self.agents.ran.lock().expect("ran lock").clone()
    }
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}
