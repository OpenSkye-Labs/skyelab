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
        tcp.set_read_timeout(Some(std::time::Duration::from_millis(20)))
            .expect("read timeout");
        let (mut ws, _) = tungstenite::client(
            format!("ws://{addr}/"),
            tungstenite::stream::MaybeTlsStream::Plain(tcp),
        )
        .expect("handshake");
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
        remote.read_with(cx, |r, _| r.status.label()).as_deref(),
        Some(format!("Sharing on desktop.example.ts.net:{port}").as_str())
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
    let label = remote
        .read_with(cx, |r, _| r.status.label())
        .expect("a label");
    assert!(label.contains("Tailscale is not running"), "{label}");
    remote.read_with(cx, |r, _| {
        assert!(r.enabled, "still wanted; the user sees why it isn't on")
    });
}

struct Fixture {
    _dir: tempfile::TempDir,
    project_dir: PathBuf,
    account_dir: PathBuf,
    store: workspace::SharedStore,
    ws: Entity<Workspace>,
    remote: Entity<Remote>,
}

/// A workspace with one project (bound to "acme" or not), sharing on, its remote calls wired.
fn sharing_workspace(bound: bool, cx: &mut TestAppContext) -> (Fixture, &mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        workspace::init(cx);
    });
    let dir = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(dir.path()).expect("canonicalize");
    let project_dir = root.join("demo");
    std::fs::create_dir_all(&project_dir).expect("project dir");
    let account_dir = root.join(".claude-acme");
    let mut config = AccountsConfig::default();
    config
        .add_account(Account {
            id: "acme".into(),
            label: "Acme Team".into(),
            config_dir: account_dir.clone(),
        })
        .expect("account");
    if bound {
        config
            .add_binding(Binding::new(project_dir.clone(), "acme".into()))
            .expect("bind");
    }
    let accounts_path = root.join("accounts.toml");
    config.save(&accounts_path).expect("save");

    // One store for the app and the daemon, with events pushed, as in main.
    let slot = DaemonSlot::default();
    let mut store = Store::open_in_memory().expect("store");
    push_events(&mut store, slot.clone());
    let store = Arc::new(Mutex::new(store));
    let (h, calls) = host(&root, Arc::default(), store.clone(), slot);
    let remote = cx.new(|cx| Remote::new(h, cx));
    let backend = workspace::Backend {
        store: store.clone(),
        claude_bin: fake_bin(),
    };
    let (remote2, folder) = (remote.clone(), root.clone());
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let mut ws = Workspace::new(
            folder,
            Some(accounts_path),
            config,
            None,
            backend,
            window,
            cx,
        );
        ws.set_remote(remote2, calls, window, cx);
        window.focus(&gpui::Focusable::focus_handle(&ws, cx), cx);
        ws
    });
    let dir_ = project_dir.clone();
    ws.update(cx, |ws, cx| ws.add_project(dir_, cx));
    wait_until(cx, "the project to load", |cx| {
        ws.read_with(cx, |ws, _| !ws.project_ids().is_empty())
    });
    remote.update(cx, |r, cx| r.toggle(cx));
    wait_until(cx, "sharing", |cx| {
        remote.read_with(cx, |r, _| matches!(r.status, Status::Sharing { .. }))
    });
    (
        Fixture {
            _dir: dir,
            project_dir,
            account_dir,
            store,
            ws,
            remote,
        },
        cx,
    )
}

fn connect_hello(f: &Fixture, cx: &mut VisualTestContext) -> Client {
    let port = f.remote.read_with(cx, |r, _| match &r.status {
        Status::Sharing { port, .. } => *port,
        other => panic!("not sharing: {other:?}"),
    });
    let client = Client::connect(port);
    client.send(
        1,
        Call::Hello {
            protocol: VERSION,
            capabilities: vec![],
        },
    );
    match client.reply(1, &mut Vec::new(), cx) {
        ServerMessage::Response {
            result: Answer::Hello(_),
            ..
        } => client,
        other => panic!("hello failed: {other:?}"),
    }
}

fn error_of(msg: ServerMessage) -> skyelab_protocol::RpcError {
    match msg {
        ServerMessage::Error { error, .. } => error,
        other => panic!("expected an error, got {other:?}"),
    }
}

#[gpui::test]
fn remote_start_runs_under_the_bound_account_shows_locally_and_pushes_events(
    cx: &mut TestAppContext,
) {
    let (f, cx) = sharing_workspace(true, cx);
    let client = connect_hello(&f, cx);
    let project_id = f.ws.read_with(cx, |ws, _| ws.project_ids()[0]);
    let mut events = Vec::new();
    client.send(
        2,
        Call::StartThread {
            project_id,
            prompt: "hello from the phone".into(),
        },
    );
    let thread_id = match client.reply(2, &mut events, cx) {
        ServerMessage::Response {
            result: Answer::ThreadStarted { thread_id },
            ..
        } => thread_id,
        other => panic!("start failed: {other:?}"),
    };
    let row = f
        .store
        .lock()
        .expect("lock")
        .thread(thread_id)
        .expect("read")
        .expect("row");
    assert_eq!(row.account_id, "acme", "the account is recorded on the row");
    assert_eq!(row.cwd, f.project_dir);
    f.ws.read_with(cx, |ws, cx| {
        assert_eq!(
            ws.open_thread_ids(cx),
            vec![thread_id],
            "a tab in the local UI"
        )
    });

    // The fake reports the CLAUDE_CONFIG_DIR it got: the bound account's, pushed to the client.
    let account = f.account_dir.to_str().expect("utf-8").to_owned();
    wait_until(cx, "the fake's env event pushed", |_| {
        while let Ok(msg) = client.inbox.try_recv() {
            if let ServerMessage::Event(e) = msg {
                events.push(e);
            }
        }
        events.iter().any(|e| e.payload_json.contains(&account))
    });
    assert!(events.iter().all(|e| e.thread_id == thread_id));
    assert!(
        events
            .iter()
            .any(|e| e.payload_json.contains("hello from the phone")),
        "the prompt event is pushed too"
    );

    // A follow-up goes through the same view and reaches the agent (the fake echoes stdin).
    client.send(
        3,
        Call::SendMessage {
            thread_id,
            text: "and again".into(),
        },
    );
    // The first prompt's session may still be starting; retry like a client would.
    let mut id = 3;
    loop {
        match client.reply(id, &mut events, cx) {
            ServerMessage::Response {
                result: Answer::Ok, ..
            } => break,
            ServerMessage::Error { error, .. } if error.message.contains("starting") && id < 50 => {
                id += 1;
                client.send(
                    id,
                    Call::SendMessage {
                        thread_id,
                        text: "and again".into(),
                    },
                );
            }
            other => panic!("send failed: {other:?}"),
        }
    }
    wait_until(cx, "the follow-up echoed", |_| {
        while let Ok(ServerMessage::Event(e)) = client.inbox.try_recv() {
            events.push(e);
        }
        events
            .iter()
            .filter(|e| e.payload_json.contains("and again"))
            .count()
            >= 2
    });

    client.send(100, Call::Interrupt { thread_id });
    assert!(matches!(
        client.reply(100, &mut events, cx),
        ServerMessage::Response {
            result: Answer::Ok,
            ..
        }
    ));
    client.send(
        101,
        Call::SendMessage {
            thread_id: 9999,
            text: "x".into(),
        },
    );
    assert_eq!(
        error_of(client.reply(101, &mut events, cx)).code,
        skyelab_protocol::ErrorCode::NotFound
    );
    client.send(
        102,
        Call::StartThread {
            project_id: 9999,
            prompt: "x".into(),
        },
    );
    assert_eq!(
        error_of(client.reply(102, &mut events, cx)).code,
        skyelab_protocol::ErrorCode::NotFound
    );
}

#[gpui::test]
fn remote_start_on_an_unbound_project_is_refused_with_no_thread(cx: &mut TestAppContext) {
    let (f, cx) = sharing_workspace(false, cx);
    let client = connect_hello(&f, cx);
    let project_id = f.ws.read_with(cx, |ws, _| ws.project_ids()[0]);
    client.send(
        2,
        Call::StartThread {
            project_id,
            prompt: "hi".into(),
        },
    );
    let error = error_of(client.reply(2, &mut Vec::new(), cx));
    assert_eq!(error.code, skyelab_protocol::ErrorCode::BadRequest);
    assert!(error.message.contains("isn't bound"), "{}", error.message);
    let threads = f
        .store
        .lock()
        .expect("lock")
        .list_threads(project_id)
        .expect("list");
    assert!(threads.is_empty(), "no thread row, no agent");
    f.ws.read_with(cx, |ws, cx| assert!(ws.open_thread_ids(cx).is_empty()));
    // Never a fallback account: the empty prompt is refused before anything too.
    client.send(
        3,
        Call::StartThread {
            project_id,
            prompt: "  ".into(),
        },
    );
    assert_eq!(
        error_of(client.reply(3, &mut Vec::new(), cx)).code,
        skyelab_protocol::ErrorCode::BadRequest
    );
}

#[gpui::test]
fn remote_disconnect_from_the_ui_closes_the_peer(cx: &mut TestAppContext) {
    let (f, cx) = sharing_workspace(true, cx);
    let client = connect_hello(&f, cx);
    f.remote.update(cx, |r, cx| r.refresh_peers(cx));
    let peer = f.remote.read_with(cx, |r, _| r.peers.clone());
    assert_eq!(peer.len(), 1);
    assert_eq!(peer[0].device, "phone.example.ts.net");
    assert_eq!(peer[0].user, "me@example.com");
    cx.dispatch_action(DisconnectPeer { id: peer[0].id });
    wait_until(cx, "the client thread to see the close", |_| {
        client.inbox.is_disconnected()
    });
    wait_until(cx, "the peer list to empty", |cx| {
        f.remote.update(cx, |r, cx| r.refresh_peers(cx));
        f.remote.read_with(cx, |r, _| r.peers.is_empty())
    });
}
