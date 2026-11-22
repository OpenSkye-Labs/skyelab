//! An in-process `skyelab-daemon` on loopback with a fake tailnet, a store on disk and an agents backend that
//! records what it is asked to do. Mirrors skyelab-client's own test harness.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use skyelab_daemon::testing::NODE_NAME;
use skyelab_daemon::testing::ca;
use skyelab_daemon::{Agents, Backend, Config, Daemon, StoreBackend};
use skyelab_protocol::{ErrorCode, Project, RpcError, Thread, ThreadEvent};
use skyelab_store::{NewThread, Store};
use skyelab_tailnet::{Cert, Error as TailnetError, Node, Tailnet, UserProfile, WhoIs, WhoIsNode};

const ME: u64 = 1001;

/// This node is 127.0.0.1; every peer is the user's own phone, or (when `tagged`) a tagged node the gate refuses.
#[derive(Default)]
pub struct FakeTailnet {
    pub tagged: AtomicBool,
}

impl Tailnet for FakeTailnet {
    fn self_node(&self) -> Result<Node, TailnetError> {
        Ok(Node {
            stable_id: "nSelf".into(),
            hostname: "desktop".into(),
            dns_name: format!("{NODE_NAME}."),
            tailscale_ips: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
            os: "macOS".into(),
            online: true,
            user_id: Some(ME),
            tags: vec![],
        })
    }

    fn peers(&self) -> Result<Vec<Node>, TailnetError> {
        Ok(vec![])
    }

    fn cert(&self, name: &str) -> Result<Cert, TailnetError> {
        Ok(ca().issue(name))
    }

    fn whois(&self, _addr: SocketAddr) -> Result<WhoIs, TailnetError> {
        let tags = if self.tagged.load(Ordering::SeqCst) {
            vec!["tag:server".into()]
        } else {
            vec![]
        };
        Ok(WhoIs {
            node: WhoIsNode {
                stable_id: "nPhone".into(),
                name: "phone.example.ts.net.".into(),
                user: Some(ME),
                tags,
            },
            user_profile: UserProfile {
                id: Some(ME),
                login_name: "me@example.com".into(),
                display_name: "Me".into(),
            },
        })
    }
}

/// What the phone asked the agents backend to do. `fail` makes every call fail like an unwired desktop.
#[derive(Default)]
pub struct Recorder {
    pub calls: Mutex<Vec<String>>,
    pub fail: AtomicBool,
}

pub struct RecordingAgents(pub Arc<Recorder>);

impl RecordingAgents {
    fn record(&self, call: String) -> Result<(), RpcError> {
        self.0.calls.lock().expect("recorder lock").push(call);
        if self.0.fail.load(Ordering::SeqCst) {
            return Err(RpcError::new(
                ErrorCode::NotWired,
                "running agents remotely is not wired up yet",
            ));
        }
        Ok(())
    }
}

impl Agents for RecordingAgents {
    fn start_thread(&self, _: i64, _: &str) -> Result<i64, RpcError> {
        Err(RpcError::new(ErrorCode::NotWired, "not in this test"))
    }
    fn send_message(&self, thread_id: i64, text: &str) -> Result<(), RpcError> {
        self.record(format!("send {thread_id} {text}"))
    }
    fn interrupt(&self, thread_id: i64) -> Result<(), RpcError> {
        self.record(format!("interrupt {thread_id}"))
    }
}

/// The store backend, except that the next `fail_lists` thread listings fail.
pub struct Flaky {
    inner: StoreBackend<RecordingAgents>,
    pub fail_lists: Arc<AtomicUsize>,
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

pub struct Env {
    _dir: tempfile::TempDir,
    pub db: PathBuf,
    pub project_id: i64,
    pub thread_id: i64,
    pub tailnet: Arc<FakeTailnet>,
    pub agents: Arc<Recorder>,
    pub fail_lists: Arc<AtomicUsize>,
    pub daemon: Option<Daemon>,
}

impl Env {
    /// A store with one project, one thread and two events, served by a daemon on a free loopback port.
    pub fn start() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = dir.path().join("store.db");
        let mut store = Store::open(&db).expect("open store");
        let project = store
            .create_project("app", Path::new("/src/app"))
            .expect("project");
        let thread = store
            .create_thread(&NewThread {
                project_id: project.id,
                title: "fix it",
                cwd: Path::new("/src/app"),
                account_id: "work",
                status: "idle",
            })
            .expect("thread");
        store
            .append_event(thread.id, "prompt", "\"fix the login\"")
            .expect("event");
        store
            .append_event(
                thread.id,
                "agent",
                r#"{"Assistant":{"content":[{"type":"text","text":"on it"}]}}"#,
            )
            .expect("event");
        let tailnet = Arc::new(FakeTailnet::default());
        let agents = Arc::new(Recorder::default());
        let fail_lists = Arc::new(AtomicUsize::new(0));
        let backend = Arc::new(Flaky {
            inner: StoreBackend::new(
                Store::open(&db).expect("open store"),
                RecordingAgents(agents.clone()),
            ),
            fail_lists: fail_lists.clone(),
        });
        let config = Config {
            port: 0,
            ..daemon_config()
        };
        let daemon = Daemon::start(config, tailnet.clone(), backend).expect("start daemon");
        Self {
            _dir: dir,
            db,
            project_id: project.id,
            thread_id: thread.id,
            tailnet,
            agents,
            fail_lists,
            daemon: Some(daemon),
        }
    }

    pub fn port(&self) -> u16 {
        self.daemon.as_ref().expect("daemon running").local_addrs()[0].port()
    }

    /// Appends an event to the store and pushes it to connected clients, as the desktop does.
    pub fn push(&self, kind: &str, payload: &str) -> ThreadEvent {
        let mut store = Store::open(&self.db).expect("open store");
        let event = skyelab_daemon::event(
            store
                .append_event(self.thread_id, kind, payload)
                .expect("append"),
        );
        self.daemon
            .as_ref()
            .expect("daemon running")
            .push_event(event.clone());
        event
    }

    pub fn calls(&self) -> Vec<String> {
        self.agents.calls.lock().expect("recorder lock").clone()
    }
}

/// The phone's test device key; [`daemon_config`] approves it for "nPhone".
pub const PHONE_KEY: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

/// A test daemon's config: quick gate, the phone already allowed.
pub fn daemon_config() -> Config {
    Config {
        gate_timeout: Duration::from_millis(500),
        approved: vec![skyelab_daemon::Approval {
            node_id: "nPhone".into(),
            key_hash: skyelab_protocol::device_key_hash(PHONE_KEY),
        }],
        ..Config::default()
    }
}

/// Client options for the loopback daemon (test-only feature): trusts the test CA and dials `NODE_NAME` on loopback,
/// so TLS still checks the name. Quick retries.
pub fn options(port: u16) -> skyelab_client::Options {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca().root()).expect("test CA root");
    let mut options = skyelab_client::Options::default()
        .allow_loopback()
        .trust_roots(roots)
        .resolve_to(IpAddr::V4(Ipv4Addr::LOCALHOST));
    options.port = port;
    options.backoff_min = Duration::from_millis(50);
    options.backoff_max = Duration::from_millis(200);
    options.device_key = Some(PHONE_KEY.into());
    options
}
