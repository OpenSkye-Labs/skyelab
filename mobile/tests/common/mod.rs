//! An in-process `skyelab-daemon` on loopback with a fake tailnet, a store on disk and an agents backend that
//! records what it is asked to do. Mirrors skyelab-client's own test harness.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use skyelab_daemon::{Agents, Config, Daemon, StoreBackend};
use skyelab_protocol::{ErrorCode, RpcError, ThreadEvent};
use skyelab_store::{NewThread, Store};
use skyelab_tailnet::{Error as TailnetError, Node, Tailnet, UserProfile, WhoIs, WhoIsNode};

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
            dns_name: "desktop.example.ts.net.".into(),
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

pub struct Env {
    _dir: tempfile::TempDir,
    pub db: PathBuf,
    pub project_id: i64,
    pub thread_id: i64,
    pub tailnet: Arc<FakeTailnet>,
    pub agents: Arc<Recorder>,
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
        let backend = Arc::new(StoreBackend::new(
            Store::open(&db).expect("open store"),
            RecordingAgents(agents.clone()),
        ));
        let config = Config {
            port: 0,
            gate_timeout: Duration::from_millis(500),
            ..Config::default()
        };
        let daemon = Daemon::start(config, tailnet.clone(), backend).expect("start daemon");
        Self {
            _dir: dir,
            db,
            project_id: project.id,
            thread_id: thread.id,
            tailnet,
            agents,
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

/// Client options for the loopback daemon: loopback allowed (test-only feature), quick retries.
pub fn options(port: u16) -> skyelab_client::Options {
    skyelab_client::Options {
        port,
        allow_loopback: true,
        backoff_min: Duration::from_millis(50),
        backoff_max: Duration::from_millis(200),
        ..skyelab_client::Options::default()
    }
}
