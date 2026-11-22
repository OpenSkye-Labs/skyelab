//! Tests against a real in-process `skyelab-daemon` (fake tailnet, loopback sockets) and hostile fake servers.
//! They live in the crate because the loopback exception only exists under `cfg(test)`.

mod backoff;
mod events;
mod guard;
mod hello;
mod hostile;
mod keepalive;
mod reconnect;
mod rpc;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use skyelab_daemon::{Backend, Config, Daemon, NotWired, StoreBackend};
use skyelab_store::{NewThread, Store};
use skyelab_tailnet::{Error as TailnetError, Node, Tailnet, UserProfile, WhoIs, WhoIsNode};

use crate::{Client, Options, Update};

const ME: u64 = 1001;

/// The daemon's view of the tailnet: this node is 127.0.0.1, every peer is the user's own phone (or whois fails).
#[derive(Default)]
pub struct FakeTailnet {
    pub whois_fails: AtomicBool,
    pub whois_calls: AtomicUsize,
}

impl FakeTailnet {
    pub fn whois_calls(&self) -> usize {
        self.whois_calls.load(Ordering::SeqCst)
    }
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
        self.whois_calls.fetch_add(1, Ordering::SeqCst);
        if self.whois_fails.load(Ordering::SeqCst) {
            return Err(TailnetError::Command {
                command: "whois",
                stderr: "no such peer".into(),
            });
        }
        Ok(WhoIs {
            node: WhoIsNode {
                stable_id: "nPhone".into(),
                name: "phone.example.ts.net.".into(),
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

/// A store on disk, so the test can append events while the daemon serves the same file.
pub struct Env {
    _dir: tempfile::TempDir,
    pub db: PathBuf,
    pub project_id: i64,
    pub thread_id: i64,
}

impl Env {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = dir.path().join("store.db");
        let store = Store::open(&db).expect("open store");
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
        Self {
            _dir: dir,
            db,
            project_id: project.id,
            thread_id: thread.id,
        }
    }

    pub fn backend(&self) -> Arc<dyn Backend> {
        Arc::new(StoreBackend::new(
            Store::open(&self.db).expect("open store"),
            NotWired,
        ))
    }

    /// Appends `n` events and returns the last one, as the daemon would push it.
    pub fn append(&self, n: usize) -> skyelab_protocol::ThreadEvent {
        let mut store = Store::open(&self.db).expect("open store");
        let mut last = None;
        for _ in 0..n {
            last = Some(
                store
                    .append_event(self.thread_id, "assistant", "{}")
                    .expect("append"),
            );
        }
        skyelab_daemon::event(last.expect("n > 0"))
    }
}

pub fn daemon_config(port: u16) -> Config {
    Config {
        port,
        gate_timeout: Duration::from_millis(500),
        ..Config::default()
    }
}

pub fn start_daemon(tailnet: Arc<FakeTailnet>, backend: Arc<dyn Backend>, port: u16) -> Daemon {
    Daemon::start(daemon_config(port), tailnet, backend).expect("start daemon")
}

/// Options for a loopback test server: loopback allowed, quick backoff.
pub fn options(port: u16) -> Options {
    Options {
        port,
        allow_loopback: true,
        backoff_min: Duration::from_millis(50),
        backoff_max: Duration::from_millis(200),
        ..Options::default()
    }
}

pub fn port(daemon: &Daemon) -> u16 {
    daemon.local_addrs()[0].port()
}

/// The next update, failing the test after 5 s.
pub fn next(client: &Client) -> Update {
    client
        .updates()
        .recv_timeout(Duration::from_secs(5))
        .expect("no update within 5 s")
}

pub fn connected(client: &Client) {
    match next(client) {
        Update::Connected(_) => {}
        other => panic!("expected Connected, got {other:?}"),
    }
}

/// Seqs of the next `n` updates, which must all be events.
pub fn seqs(client: &Client, n: usize) -> Vec<i64> {
    (0..n)
        .map(|_| match next(client) {
            Update::Event(ev) => ev.seq,
            other => panic!("expected an event, got {other:?}"),
        })
        .collect()
}

/// Nothing more arrives for `d`.
pub fn quiet(client: &Client, d: Duration) {
    if let Ok(update) = client.updates().recv_timeout(d) {
        panic!("expected no update, got {update:?}");
    }
}
