//! Sharing on/off with a fake Tailnet, and remote agent calls running through the thread view, end to end over
//! a real daemon on 127.0.0.1. The agent is the fake claude; no real account is ever used.

use super::*;
use crate::workspace::{self, Workspace};
use gpui::{Entity, TestAppContext, VisualTestContext};
use skyelab_core::{Account, AccountsConfig, Binding};
use skyelab_protocol::{Call, Reply as Answer, Request, ServerMessage, VERSION};
use skyelab_tailnet::{Error, Node, Tailnet, UserProfile, WhoIs, WhoIsNode};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tungstenite::Message;

const ME: u64 = 7;

/// This node is 127.0.0.1; every peer is the user's own phone, unless the tailnet is down.
#[derive(Default)]
struct FakeTailnet {
    down: AtomicBool,
}

impl Tailnet for FakeTailnet {
    fn self_node(&self) -> Result<Node, Error> {
        if self.down.load(Ordering::SeqCst) {
            return Err(Error::NotRunning("Stopped".into()));
        }
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
    fn peers(&self) -> Result<Vec<Node>, Error> {
        Ok(vec![])
    }
    fn whois(&self, _: SocketAddr) -> Result<WhoIs, Error> {
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

fn fake_bin() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh")
}

fn host(
    dir: &Path,
    tailnet: Arc<FakeTailnet>,
    store: workspace::SharedStore,
    slot: DaemonSlot,
) -> (Host, flume::Receiver<RemoteCall>) {
    let (agents, calls) = UiAgents::new();
    let host = Host {
        settings_path: Some(dir.join("settings.toml")),
        tailnet,
        config: Config {
            port: 0,
            ..Config::default()
        },
        backend: Arc::new(skyelab_daemon::StoreBackend::shared(store, agents)),
        slot,
    };
    (host, calls)
}

/// Pumps the app until `done`, giving the daemon thread and the fake agent real time.
fn wait_until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..1000 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

fn sharing_port(remote: &Entity<Remote>, cx: &mut TestAppContext) -> Option<u16> {
    remote.read_with(cx, |r, _| match &r.status {
        Status::Sharing { port, .. } => Some(*port),
        _ => None,
    })
}

/// A client on its own thread: the daemon's replies need the test thread to keep pumping the app.
struct Client {
    out: flume::Sender<Request>,
    inbox: flume::Receiver<ServerMessage>,
}

impl Client {
    fn connect(port: u16) -> Self {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let tcp = TcpStream::connect(addr).expect("connect");
        let (mut ws, _) = tungstenite::client(
            format!("ws://{addr}/"),
            tungstenite::stream::MaybeTlsStream::Plain(tcp),
        )
        .expect("handshake");
        // Short reads after the handshake, so the loop can also send.
        if let tungstenite::stream::MaybeTlsStream::Plain(tcp) = ws.get_ref() {
            tcp.set_read_timeout(Some(std::time::Duration::from_millis(20)))
                .expect("read timeout");
        }
        let (out, outbox) = flume::unbounded::<Request>();
        let (inbox_tx, inbox) = flume::unbounded();
        std::thread::spawn(move || {
            loop {
                while let Ok(req) = outbox.try_recv() {
                    let text = serde_json::to_string(&req).expect("request serializes");
                    if ws.send(Message::text(text)).is_err() {
                        return;
                    }
                }
                match ws.read() {
                    Ok(Message::Text(t)) => {
                        let msg = serde_json::from_str(&t).expect("server message parses");
                        if inbox_tx.send(msg).is_err() {
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e))
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => return, // closed
                }
            }
        });
        Self { out, inbox }
    }

    fn send(&self, id: u64, call: Call) {
        self.out
            .send(Request { id, call })
            .expect("client thread alive");
    }

    /// Pumps the app until the reply to `id` arrives; events seen meanwhile go to `events`.
    fn reply(
        &self,
        id: u64,
        events: &mut Vec<skyelab_protocol::ThreadEvent>,
        cx: &mut VisualTestContext,
    ) -> ServerMessage {
        let mut got = None;
        wait_until(cx, "a reply", |_| {
            while let Ok(msg) = self.inbox.try_recv() {
                match msg {
                    ServerMessage::Event(e) => events.push(e),
                    m @ (ServerMessage::Response { id: i, .. }
                    | ServerMessage::Error { id: Some(i), .. })
                        if i == id =>
                    {
                        got = Some(m)
                    }
                    other => panic!("unexpected {other:?}"),
                }
            }
            got.is_some()
        });
        got.expect("waited for it")
    }
}

#[gpui::test]
fn remote_toggle_starts_and_stops_the_daemon_and_persists(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let cx = cx.add_empty_window();
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("store")));
    let (h, _calls) = host(
        dir.path(),
        Arc::default(),
        store.clone(),
        DaemonSlot::default(),
    );
    let remote = cx.new(|cx| Remote::new(h, cx));
    remote.read_with(cx, |r, _| {
        assert!(!r.enabled, "off by default");
        assert_eq!(r.status, Status::Off);
    });

    remote.update(cx, |r, cx| r.toggle(cx));
    wait_until(cx, "sharing", |cx| sharing_port(&remote, cx).is_some());
    let port = sharing_port(&remote, cx).expect("sharing");
    assert_eq!(
        remote.read_with(cx, |r, _| r.status.label()),
        format!("Sharing on desktop.example.ts.net:{port}")
    );
    let saved = AppSettings::load(&dir.path().join("settings.toml")).expect("settings");
    assert!(saved.share_on_tailnet, "the toggle is saved");
    assert!(
        !dir.path().join("accounts.toml").exists(),
        "never in accounts.toml"
    );
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    assert!(TcpStream::connect(addr).is_ok(), "listening");

    remote.update(cx, |r, cx| r.toggle(cx));
    remote.read_with(cx, |r, _| assert_eq!(r.status, Status::Off));
    wait_until(cx, "the listener to close", |_| {
        TcpStream::connect(addr).is_err()
    });
    assert!(
        !AppSettings::load(&dir.path().join("settings.toml"))
            .expect("settings")
            .share_on_tailnet
    );

    // Left on, it starts again at the next launch.
    AppSettings {
        share_on_tailnet: true,
        ..AppSettings::default()
    }
    .save(&dir.path().join("settings.toml"))
    .expect("save");
    let (h, _calls) = host(dir.path(), Arc::default(), store, DaemonSlot::default());
    let relaunched = cx.new(|cx| Remote::new(h, cx));
    wait_until(cx, "sharing after relaunch", |cx| {
        sharing_port(&relaunched, cx).is_some()
    });
}

#[gpui::test]
fn remote_toggle_shows_the_tailnet_error(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let cx = cx.add_empty_window();
    let dir = tempfile::tempdir().expect("tempdir");
    let tailnet = Arc::new(FakeTailnet::default());
    tailnet.down.store(true, Ordering::SeqCst);
    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("store")));
    let (h, _calls) = host(dir.path(), tailnet, store, DaemonSlot::default());
    let remote = cx.new(|cx| Remote::new(h, cx));
    remote.update(cx, |r, cx| r.toggle(cx));
    wait_until(cx, "the start to fail", |cx| {
        remote.read_with(cx, |r, _| matches!(r.status, Status::Failed(_)))
    });
    let label = remote.read_with(cx, |r, _| r.status.label());
    assert!(label.contains("Tailscale is not running"), "{label}");
    remote.read_with(cx, |r, _| {
        assert!(r.enabled, "still wanted; the user sees why it isn't on")
    });
}

#[gpui::test]
fn remote_off_stops_sharing_even_when_the_setting_cant_be_saved(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let cx = cx.add_empty_window();
    let dir = tempfile::tempdir().expect("tempdir");
    let settings = dir.path().join("settings.toml");
    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("store")));
    let (h, _calls) = host(dir.path(), Arc::default(), store, DaemonSlot::default());
    let remote = cx.new(|cx| Remote::new(h, cx));
    remote.update(cx, |r, cx| r.toggle(cx));
    wait_until(cx, "sharing", |cx| sharing_port(&remote, cx).is_some());
    let addr = SocketAddr::from((
        Ipv4Addr::LOCALHOST,
        sharing_port(&remote, cx).expect("port"),
    ));

    std::fs::write(&settings, "share_on_tailnet = [[[").expect("corrupt");
    remote.update(cx, |r, cx| r.toggle(cx));
    wait_until(cx, "the listener to close", |_| {
        TcpStream::connect(addr).is_err()
    });
    remote.read_with(cx, |r, _| {
        assert!(!r.enabled);
        let label = r.status.label();
        assert!(
            label.contains("parsing"),
            "the save error is shown: {label}"
        );
    });
    assert_eq!(
        std::fs::read_to_string(&settings).expect("read"),
        "share_on_tailnet = [[[",
        "a file that doesn't parse is left alone"
    );
    // Turning on can't be saved either, so it doesn't start.
    remote.update(cx, |r, cx| r.toggle(cx));
    cx.run_until_parked();
    remote.read_with(cx, |r, _| {
        assert!(!r.enabled && !matches!(r.status, Status::Starting | Status::Sharing { .. }))
    });
}

#[gpui::test]
fn remote_dropping_the_entity_stops_the_daemon(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let cx = cx.add_empty_window();
    let dir = tempfile::tempdir().expect("tempdir");
    let slot = DaemonSlot::default();
    let mut store = Store::open_in_memory().expect("store");
    push_events(&mut store, slot.clone());
    let store = Arc::new(Mutex::new(store));
    let (h, _calls) = host(dir.path(), Arc::default(), store, slot.clone());
    let remote = cx.new(|cx| Remote::new(h, cx));
    remote.update(cx, |r, cx| r.toggle(cx));
    wait_until(cx, "sharing", |cx| sharing_port(&remote, cx).is_some());
    let addr = SocketAddr::from((
        Ipv4Addr::LOCALHOST,
        sharing_port(&remote, cx).expect("port"),
    ));
    // The window closing releases the last handle, as Workspace and the panel go away.
    drop(remote);
    cx.update(|_, _| {}); // releases happen when effects flush
    wait_until(cx, "the listener to close", |_| {
        TcpStream::connect(addr).is_err()
    });
    assert!(lock_slot(&slot).is_none());
}

#[gpui::test]
fn remote_off_then_on_quickly_ends_up_sharing(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let cx = cx.add_empty_window();
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("store")));
    let (mut h, _calls) = host(dir.path(), Arc::default(), store, DaemonSlot::default());
    // A fixed port, as in the app: the old listener must be gone before the new one binds.
    let free = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    h.config.port = free.local_addr().expect("addr").port();
    drop(free);
    let remote = cx.new(|cx| Remote::new(h, cx));
    remote.update(cx, |r, cx| r.toggle(cx));
    wait_until(cx, "sharing", |cx| sharing_port(&remote, cx).is_some());
    for _ in 0..3 {
        remote.update(cx, |r, cx| {
            r.toggle(cx);
            r.toggle(cx);
        });
        wait_until(cx, "sharing again", |cx| {
            let status = remote.read_with(cx, |r, _| r.status.clone());
            assert!(!matches!(status, Status::Failed(_)), "{status:?}");
            matches!(status, Status::Sharing { .. })
        });
    }
}

mod calls;
