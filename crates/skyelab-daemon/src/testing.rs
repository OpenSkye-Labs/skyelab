//! The remote-test harness every crate shares (feature `test-support`): test certificates, a blocking TLS client,
//! a configurable fake tailnet, and an [`Env`] that serves a store through an in-process daemon on loopback.
//! Never enabled in a shipped build.

use std::net::{SocketAddr, TcpStream};
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
use skyelab_tailnet::Cert;

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
    tcp.set_read_timeout(Some(Duration::from_secs(30)))?;
    let mut tls = StreamOwned::new(conn, tcp);
    // Finish the handshake here, so a refused certificate fails this call, not the first WebSocket read.
    while tls.conn.is_handshaking() {
        tls.conn.complete_io(&mut tls.sock)?;
    }
    Ok(tls)
}

/// The tailnet user every fake node belongs to unless a test says otherwise.
pub const ME: u64 = 1001;

/// The test client's device key. [`config`] approves it for the phone ("nPhone"), the node whois names by default.
/// `skyelab_client::testing::DEVICE_KEY` is the same key (a client test checks they agree).
pub const DEVICE_KEY: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

/// [`DEVICE_KEY`] approved on "nPhone".
pub fn approvals() -> Vec<crate::Approval> {
    vec![crate::Approval {
        node_id: "nPhone".into(),
        key_hash: skyelab_protocol::device_key_hash(DEVICE_KEY),
    }]
}

mod tailnet;
pub use tailnet::{FakeTailnet, Peer, node};

/// Daemon settings for tests: `port` (0 picks one), the default gate timeout, and the test key approved
/// ([`approvals`]). The gate covers whois and the TLS handshake, so a short one rejects good clients on a loaded
/// machine: only tests about the timeout shorten it.
pub fn config(port: u16) -> Config {
    Config {
        port,
        approved: approvals(),
        ..Config::default()
    }
}

/// What the agents backend was asked to run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ran {
    Start(i64, String),
    Send(i64, String),
    Interrupt(i64),
    Answer(i64, String, skyelab_protocol::PermissionAnswer),
    Questions(i64, String, Vec<Vec<String>>),
    Shelve(i64, bool, Option<i64>),
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
    fn answer_permission(
        &self,
        thread_id: i64,
        request_id: &str,
        decision: skyelab_protocol::PermissionAnswer,
    ) -> Result<(), RpcError> {
        self.record(Ran::Answer(thread_id, request_id.into(), decision))
    }
    fn answer_questions(
        &self,
        thread_id: i64,
        request_id: &str,
        answers: &[Vec<String>],
    ) -> Result<(), RpcError> {
        self.record(Ran::Questions(
            thread_id,
            request_id.into(),
            answers.to_vec(),
        ))
    }
    /// Writes the store as the app does, so a listing shows the shelf.
    fn shelve_thread(
        &self,
        thread_id: i64,
        settled: bool,
        snoozed_until: Option<i64>,
    ) -> Result<(), RpcError> {
        self.record(Ran::Shelve(thread_id, settled, snoozed_until))?;
        let store = self.store.lock().expect("store lock");
        let written = if settled {
            store.set_thread_settled(thread_id, true)
        } else {
            store
                .set_thread_settled(thread_id, false)
                .and_then(|()| store.set_thread_snoozed(thread_id, snoozed_until))
        };
        written.map_err(|e| RpcError::new(ErrorCode::NotFound, e.to_string()))
    }
}

/// The store backend, except that the next `fail_lists` thread listings fail. Every call is logged in `asked`.
struct Flaky {
    inner: StoreBackend<RecordingAgents>,
    fail_lists: Arc<AtomicUsize>,
    asked: Arc<Mutex<Vec<&'static str>>>,
}

impl Flaky {
    fn ask(&self, call: &'static str) {
        self.asked.lock().expect("asked lock").push(call);
    }
}

impl Backend for Flaky {
    fn list_projects(&self) -> Result<Vec<Project>, RpcError> {
        self.ask("list_projects");
        self.inner.list_projects()
    }
    fn list_threads(&self, project_id: i64) -> Result<Vec<Thread>, RpcError> {
        self.ask("list_threads");
        let failing = self
            .fail_lists
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
        if failing.is_ok() {
            return Err(RpcError::new(ErrorCode::Internal, "store busy"));
        }
        self.inner.list_threads(project_id)
    }
    fn thread_events_since(&self, thread_id: i64, seq: i64) -> Result<Vec<ThreadEvent>, RpcError> {
        self.ask("thread_events_since");
        self.inner.thread_events_since(thread_id, seq)
    }
    fn start_thread(&self, project_id: i64, prompt: &str) -> Result<i64, RpcError> {
        self.ask("start_thread");
        self.inner.start_thread(project_id, prompt)
    }
    fn send_message(&self, thread_id: i64, text: &str) -> Result<(), RpcError> {
        self.ask("send_message");
        self.inner.send_message(thread_id, text)
    }
    fn interrupt(&self, thread_id: i64) -> Result<(), RpcError> {
        self.ask("interrupt");
        self.inner.interrupt(thread_id)
    }
    fn answer_permission(
        &self,
        thread_id: i64,
        request_id: &str,
        decision: skyelab_protocol::PermissionAnswer,
    ) -> Result<(), RpcError> {
        self.ask("answer_permission");
        self.inner
            .answer_permission(thread_id, request_id, decision)
    }
    fn answer_questions(
        &self,
        thread_id: i64,
        request_id: &str,
        answers: &[Vec<String>],
    ) -> Result<(), RpcError> {
        self.ask("answer_questions");
        self.inner.answer_questions(thread_id, request_id, answers)
    }
    fn shelve_thread(
        &self,
        thread_id: i64,
        settled: bool,
        snoozed_until: Option<i64>,
    ) -> Result<(), RpcError> {
        self.ask("shelve_thread");
        self.inner.shelve_thread(thread_id, settled, snoozed_until)
    }
    fn subscribe_shell(&self) -> Result<crate::ShellFeed, RpcError> {
        self.ask("subscribe_shell");
        self.inner.subscribe_shell()
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
    /// Every backend call any client made, by name, across restarts. See [`Env::asked`].
    pub asked_log: Arc<Mutex<Vec<&'static str>>>,
    /// Where `serve` listens: picked by the first `serve`, then the same on every restart. 0 until then.
    pub port: u16,
    pub daemon: Option<Daemon>,
    /// This machine's accounts.toml, for account labels. Takes effect on the next `serve`.
    pub accounts: Option<PathBuf>,
    cwd: PathBuf,
    /// Holds the store; tests may put other files in it. Last, so the daemon and store let go first.
    pub _dir: tempfile::TempDir,
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
            asked_log: Arc::default(),
            port: 0,
            daemon: None,
            accounts: None,
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
                self.accounts.clone(),
            ),
            fail_lists: self.fail_lists.clone(),
            asked: self.asked_log.clone(),
        })
    }

    /// (Re)starts the daemon: on a free port the first time, then on that same port.
    pub fn serve(&mut self) {
        self.daemon = None;
        let backend = self.backend();
        let mut last = None;
        // Daemon::start already waits up to 0.5 s for a busy port; this makes it about 2 s in all.
        for _ in 0..4 {
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

    /// The backend calls served so far (`list_projects`, `thread_events_since`, ...), oldest first.
    pub fn asked(&self) -> Vec<&'static str> {
        self.asked_log.lock().expect("asked lock").clone()
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
