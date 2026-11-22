//! Test harness for the environment switcher: a "remote" SkyeLab (an in-process daemon over a temp-file store, a
//! fake tailnet that admits us) and a local workspace with its own empty store. No agent runs on either side.

use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_daemon::testing::{NODE_NAME, ca};
use skyelab_daemon::{Agents, Config, Daemon, StoreBackend};
use skyelab_protocol::RpcError;
use skyelab_tailnet::{Cert, Error as TailnetError, Node, Tailnet, UserProfile, WhoIs, WhoIsNode};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(super) const ME: u64 = 42;

pub(super) fn node(id: &str, dns: &str, user: u64, online: bool, tags: &[&str]) -> Node {
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

/// This node is 127.0.0.1. `peers` is what `tailscale status` lists; whois admits everyone unless `refuse`.
#[derive(Default)]
pub(super) struct FakeTailnet {
    pub peers: Vec<Node>,
    pub refuse: AtomicBool,
}

impl Tailnet for FakeTailnet {
    fn self_node(&self) -> Result<Node, TailnetError> {
        Ok(node("nSelf", NODE_NAME, ME, true, &[]))
    }
    fn peers(&self) -> Result<Vec<Node>, TailnetError> {
        Ok(self.peers.clone())
    }
    fn cert(&self, name: &str) -> Result<Cert, TailnetError> {
        Ok(ca().issue(name))
    }
    fn whois(&self, _: SocketAddr) -> Result<WhoIs, TailnetError> {
        if self.refuse.load(Ordering::SeqCst) {
            return Err(TailnetError::Command {
                command: "whois",
                stderr: "no such peer".into(),
            });
        }
        Ok(WhoIs {
            node: WhoIsNode {
                stable_id: "nLaptop".into(),
                name: "laptop.example.ts.net.".into(),
                user: Some(ME),
                tags: vec![],
            },
            user_profile: UserProfile {
                id: Some(ME),
                login_name: "me@example.com".into(),
                display_name: "Me".into(),
            },
        })
    }
}

/// What the remote backend was asked to run.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Ran {
    Start(i64, String),
    Send(i64, String),
    Interrupt(i64),
}

/// Records agent calls; a start creates the thread row (account "work") so it can be listed and watched.
pub(super) struct Recorder {
    pub store: SharedStore,
    pub ran: Arc<Mutex<Vec<Ran>>>,
}

impl Agents for Recorder {
    fn start_thread(&self, project_id: i64, prompt: &str) -> Result<i64, RpcError> {
        self.ran
            .lock()
            .expect("ran lock")
            .push(Ran::Start(project_id, prompt.into()));
        let mut store = self.store.lock().expect("store lock");
        let thread = store
            .create_thread(&skyelab_store::NewThread {
                project_id,
                title: prompt,
                cwd: Path::new("/remote/app"),
                account_id: "work",
                status: "running",
            })
            .expect("thread");
        let (kind, payload) = super::thread_model::Record::Prompt(prompt.into()).to_row();
        store
            .append_event(thread.id, kind, &payload)
            .expect("append");
        Ok(thread.id)
    }
    fn send_message(&self, thread_id: i64, text: &str) -> Result<(), RpcError> {
        self.ran
            .lock()
            .expect("ran lock")
            .push(Ran::Send(thread_id, text.into()));
        Ok(())
    }
    fn interrupt(&self, thread_id: i64) -> Result<(), RpcError> {
        self.ran
            .lock()
            .expect("ran lock")
            .push(Ran::Interrupt(thread_id));
        Ok(())
    }
}

/// The other machine: its store (one project "app" with one thread on account "work") and its daemon.
pub(super) struct RemoteSide {
    pub _dir: tempfile::TempDir,
    pub store: SharedStore,
    pub tailnet: Arc<FakeTailnet>,
    pub ran: Arc<Mutex<Vec<Ran>>>,
    pub daemon: Option<Daemon>,
    pub port: u16,
    pub project_id: i64,
    pub thread_id: i64,
}

impl RemoteSide {
    pub fn new(tailnet: FakeTailnet) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("remote.db");
        let store = Store::open(&db).expect("store");
        let project = store
            .create_project("app", Path::new("/remote/app"))
            .expect("project");
        let thread = store
            .create_thread(&skyelab_store::NewThread {
                project_id: project.id,
                title: "fix the login bug",
                cwd: Path::new("/remote/app"),
                account_id: "work",
                status: "idle",
            })
            .expect("thread");
        let free = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = free.local_addr().expect("addr").port();
        drop(free);
        let mut this = Self {
            _dir: dir,
            store: Arc::new(Mutex::new(store)),
            tailnet: Arc::new(tailnet),
            ran: Arc::default(),
            daemon: None,
            port,
            project_id: project.id,
            thread_id: thread.id,
        };
        this.start();
        this
    }

    /// (Re)starts the daemon on the same port.
    pub fn start(&mut self) {
        let backend = Arc::new(StoreBackend::shared(
            self.store.clone(),
            Recorder {
                store: self.store.clone(),
                ran: self.ran.clone(),
            },
        ));
        let config = Config {
            port: self.port,
            gate_timeout: Duration::from_millis(500),
            ..Config::default()
        };
        for _ in 0..100 {
            match Daemon::start(config.clone(), self.tailnet.clone(), backend.clone()) {
                Ok(d) => {
                    self.daemon = Some(d);
                    return;
                }
                // The old listener's port can take a moment to free.
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        panic!("daemon didn't start on port {}", self.port);
    }

    pub fn stop(&mut self) {
        self.daemon = None;
    }

    /// Stores agent text events on the thread. With `push`, pushes only the last one, so a client must fill the rest.
    pub fn store_texts(&self, thread_id: i64, texts: &[&str], push: bool) {
        let mut last = None;
        {
            let mut store = self.store.lock().expect("store lock");
            for text in texts {
                let record = super::thread_model::Record::Agent(skyelab_agent::Event::Assistant {
                    content: vec![skyelab_agent::Block::Text {
                        text: (*text).to_owned(),
                    }],
                });
                let (kind, payload) = record.to_row();
                last = Some(
                    store
                        .append_event(thread_id, kind, &payload)
                        .expect("append"),
                );
            }
        }
        if push && let (Some(last), Some(daemon)) = (last, &self.daemon) {
            daemon.push_event(skyelab_daemon::event(last));
        }
    }

    pub fn ran(&self) -> Vec<Ran> {
        self.ran.lock().expect("ran lock").clone()
    }

    pub fn options(&self) -> skyelab_client::Options {
        let mut options = test_options();
        options.port = self.port;
        options.backoff_min = Duration::from_millis(50);
        options.backoff_max = Duration::from_millis(200);
        options
    }
}

pub(super) fn setup(cx: &mut TestAppContext) {
    // The client and daemon threads wake the UI.
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
}

/// A local workspace with an empty store of its own, wired to `tailnet` for the picker.
pub(super) fn local_workspace(
    tailnet: Arc<FakeTailnet>,
    settings: Option<PathBuf>,
    options: skyelab_client::Options,
    cx: &mut TestAppContext,
) -> (Entity<Workspace>, SharedStore, &mut VisualTestContext) {
    setup(cx);
    let store: SharedStore = Arc::new(Mutex::new(Store::open_in_memory().expect("store")));
    let backend = Backend {
        store: store.clone(),
        claude_bin: Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh"),
    };
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let mut ws = Workspace::new(
            std::env::temp_dir(),
            None,
            AccountsConfig::default(),
            None,
            backend,
            window,
            cx,
        );
        ws.set_environments(tailnet, settings, options, window, cx);
        ws
    });
    ws.update_in(cx, |ws, window, cx| ws.focus.clone().focus(window, cx));
    (ws, store, cx)
}

/// Pumps the app until `done`, giving the client and daemon threads real time.
pub(super) fn wait_until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..800 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

pub(super) fn switch(ws: &Entity<Workspace>, host: Option<&str>, cx: &mut VisualTestContext) {
    let action = env_mode::SwitchEnvironment {
        host: host.map(str::to_owned),
    };
    ws.update_in(cx, |ws, window, cx| {
        ws.switch_environment(&action, window, cx)
    });
}

pub(super) fn state(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> String {
    ws.read_with(cx, |ws, cx| {
        ws.remote_env()
            .map(|e| e.read(cx).state.label().to_string())
            .unwrap_or_default()
    })
}

/// The sidebar's sections as (title, row labels).
pub(super) fn sidebar(
    ws: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Vec<(String, Vec<String>)> {
    ws.read_with(cx, |ws, cx| {
        ws.sidebar
            .read(cx)
            .sections
            .iter()
            .map(|s| {
                (
                    s.title.to_string(),
                    s.items.iter().map(|i| i.label.to_string()).collect(),
                )
            })
            .collect()
    })
}

/// Clicks the first sidebar row labelled `label` (dispatches its action on the workspace).
pub(super) fn click_row(ws: &Entity<Workspace>, label: &str, cx: &mut VisualTestContext) {
    let action = ws.read_with(cx, |ws, cx| {
        ws.sidebar
            .read(cx)
            .sections
            .iter()
            .flat_map(|s| &s.items)
            .find(|i| i.label == label)
            .map(|i| i.action.boxed_clone())
            .unwrap_or_else(|| panic!("no sidebar row {label}"))
    });
    ws.update_in(cx, |_, window, cx| window.dispatch_action(action, cx));
}

/// Connected to `remote` and showing its projects.
pub(super) fn connect(ws: &Entity<Workspace>, remote: &RemoteSide, cx: &mut VisualTestContext) {
    let _ = remote;
    switch(ws, Some(NODE_NAME), cx);
    wait_until(cx, "connected", |cx| state(ws, cx) == "Connected");
    wait_until(cx, "remote projects", |cx| {
        sidebar(ws, cx)
            .first()
            .is_some_and(|(_, rows)| rows == &["app".to_owned()])
    });
}

/// A client that trusts the test CA and dials this node's MagicDNS name on loopback (TLS still checks the name).
pub(super) fn test_options() -> skyelab_client::Options {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca().root()).expect("test CA root");
    skyelab_client::Options::default()
        .allow_loopback()
        .trust_roots(roots)
        .resolve_to(IpAddr::V4(Ipv4Addr::LOCALHOST))
}
