//! Acceptance: protocol 1.7 `subscribe_shell` delivers live thread creation, updates and removal to mobile without
//! polling or manual Back+refresh; < 1.7 servers fall back to `list_projects` + `list_threads` without errors;
//! refusal, `IdentityMismatch` and `VersionMismatch` clear projects, threads and events immediately.

pub mod common;
#[path = "common/tls_ws.rs"]
mod tls_ws;

use std::net::TcpListener;
use std::path::Path;
use std::time::{Duration, Instant};

use common::{NODE_NAME, options, start};
use gpui::TestAppContext;
use skyelab_client::{Disconnect, DisconnectKind, Error, Update};
use skyelab_mobile::{
    app::{MobileApp, Screen},
    remote::RemoteSource,
    source::{Connection, Msg, ThreadSource},
};
use skyelab_protocol::{Call, Hello, Project, Reply, Request, ServerMessage, Thread, Version};
use skyelab_store::NewThread;
use tungstenite::Message;

fn settle(source: &mut RemoteSource, what: &str, done: impl Fn(&RemoteSource) -> bool) {
    let inbox = source.inbox().expect("remote inbox");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(source) {
        let left = deadline.saturating_duration_since(Instant::now());
        match inbox.recv_timeout(left) {
            Ok(msg) => source.apply(msg),
            Err(_) => panic!(
                "timed out waiting for {what}; connection {:?}, error {:?}",
                source.connection(),
                source.error()
            ),
        }
    }
}

#[gpui::test]
fn shell_stream_updates_mobile_thread_list_live(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_component::init);
    let env = start();
    let port = env.port;
    let dir = tempfile::tempdir().expect("temp dir");
    let data = dir.path().to_path_buf();
    let (app, cx) = cx.add_window_view(|window, cx| {
        let pins = data.clone();
        let connect = Box::new(move |_: &str| {
            Ok(
                Box::new(RemoteSource::connect(NODE_NAME, options(port), &pins)?)
                    as Box<dyn ThreadSource>,
            )
        });
        MobileApp::new(data.clone(), connect, window, cx)
    });
    let env_input = app.read_with(cx, |a, _| a.env_input.clone());
    env_input.update_in(cx, |s, window, cx| s.set_value(NODE_NAME, window, cx));
    app.update_in(cx, |a, window, cx| a.connect(window, cx));

    let wait = |cx: &mut gpui::VisualTestContext, what: &str, done: &dyn Fn(&MobileApp) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            cx.run_until_parked();
            if app.read_with(cx, |a, _| done(a)) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };

    wait(cx, "initial snapshot", &|a| {
        a.threads().iter().any(|t| t.title == "fix it")
    });
    let asked = env.asked();
    assert!(asked.contains(&"subscribe_shell"), "asked: {asked:?}");
    assert!(
        !asked.contains(&"list_projects") && !asked.contains(&"list_threads"),
        "1.7 must use subscribe_shell, not legacy listing: {asked:?}"
    );

    // 1. Live creation on the desktop appears without Back or manual refresh.
    let created_id = {
        let store = env.store.lock().expect("store lock");
        store
            .create_thread(&NewThread {
                project_id: env.project_id,
                title: "started on desktop",
                cwd: Path::new("/src/app"),
                account_id: "work",
                status: "running",
            })
            .expect("create thread")
            .id
    };
    wait(cx, "live created thread", &|a| {
        a.threads()
            .iter()
            .any(|t| t.id == created_id && t.title == "started on desktop" && t.status == "running")
    });

    // 2. Live update (rename + status change) appears immediately.
    {
        let store = env.store.lock().expect("store lock");
        store
            .set_thread_title(created_id, "renamed live")
            .expect("rename");
        store.set_thread_status(created_id, "idle").expect("status");
    }
    wait(cx, "live updated thread", &|a| {
        a.threads()
            .iter()
            .any(|t| t.id == created_id && t.title == "renamed live" && t.status == "idle")
    });

    // 3. Live removal disappears immediately.
    {
        let store = env.store.lock().expect("store lock");
        store.delete_thread(created_id).expect("delete");
    }
    wait(cx, "live removed thread", &|a| {
        !a.threads().iter().any(|t| t.id == created_id)
    });
}

/// A mock server speaking protocol 1.`minor` (< 1.7): serves `list_projects` and `list_threads` and records calls.
fn legacy_server(minor: u32) -> (u16, flume::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let (seen_tx, seen_rx) = flume::unbounded();
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let Some(mut ws) = tls_ws::accept(stream) else {
            return;
        };
        while let Ok(msg) = ws.read() {
            let Message::Text(text) = msg else { continue };
            let req: Request = serde_json::from_str(&text).expect("request");
            let method = match &req.call {
                Call::Hello { .. } => "hello",
                Call::ListProjects => "list_projects",
                Call::ListThreads { .. } => "list_threads",
                Call::SubscribeShell => "subscribe_shell",
                _ => "other",
            };
            let _ = seen_tx.send(method.to_owned());
            let reply = match req.call {
                Call::Hello { .. } => ServerMessage::Response {
                    id: req.id,
                    result: Reply::Hello(Hello {
                        protocol: Version { major: 1, minor },
                        capabilities: vec![],
                        node_id: None,
                    }),
                },
                Call::ListProjects => ServerMessage::Response {
                    id: req.id,
                    result: Reply::Projects(vec![Project {
                        id: 1,
                        name: "legacy-app".into(),
                        path: "/src/legacy".into(),
                        created_at: 0,
                    }]),
                },
                Call::ListThreads { project_id } => ServerMessage::Response {
                    id: req.id,
                    result: Reply::Threads(vec![Thread {
                        id: 42,
                        project_id,
                        title: format!("from 1.{minor}"),
                        account_id: "work".into(),
                        account_label: Some("Work".into()),
                        status: "idle".into(),
                        pinned: false,
                        preview: None,
                        settled_at: None,
                        snoozed_until: None,
                        created_at: 100,
                        updated_at: 200,
                        row: Default::default(),
                    }]),
                },
                _ => ServerMessage::Response {
                    id: req.id,
                    result: Reply::Ok,
                },
            };
            if ws.send(Message::text(reply.to_json())).is_err() {
                break;
            }
        }
    });
    (port, seen_rx)
}

#[test]
fn servers_older_than_1_7_fall_back_to_listing_without_errors() {
    for minor in [4, 6] {
        let (port, seen) = legacy_server(minor);
        let pins = tempfile::tempdir().expect("pin dir");
        let mut source =
            RemoteSource::connect(NODE_NAME, options(port), pins.path()).expect("client");
        settle(&mut source, "legacy threads", |s| !s.threads().is_empty());
        assert_eq!(source.connection(), Connection::Connected);
        assert_eq!(source.error(), None);
        assert_eq!(source.projects()[0].name, "legacy-app");
        assert_eq!(source.threads()[0].title, format!("from 1.{minor}"));
        let calls: Vec<String> = seen.try_iter().collect();
        assert!(
            !calls.iter().any(|m| m == "subscribe_shell"),
            "1.{minor} must never be asked subscribe_shell: {calls:?}"
        );
        assert!(calls.iter().any(|m| m == "list_projects"), "{calls:?}");
        assert!(calls.iter().any(|m| m == "list_threads"), "{calls:?}");
    }
}

#[test]
fn version_mismatch_and_identity_mismatch_clear_shell_state() {
    let env = start();
    let pins = tempfile::tempdir().expect("pin dir");
    let mut source =
        RemoteSource::connect(NODE_NAME, options(env.port), pins.path()).expect("client");
    settle(&mut source, "snapshot", |s| !s.threads().is_empty());
    source.watch(env.thread_id);
    settle(&mut source, "events", |s| {
        s.events(env.thread_id).len() == 2
    });

    source.apply(Msg::from(Update::Disconnected(Disconnect {
        kind: DisconnectKind::IdentityMismatch,
        message: "node id mismatch".into(),
    })));
    assert!(matches!(source.connection(), Connection::Unverified(_)));
    assert!(source.projects().is_empty());
    assert!(source.threads().is_empty());
    assert!(source.events(env.thread_id).is_empty());

    // Reconnect and check VersionMismatch clears everything too.
    let pins2 = tempfile::tempdir().expect("pin dir 2");
    let mut source2 =
        RemoteSource::connect(NODE_NAME, options(env.port), pins2.path()).expect("client");
    settle(&mut source2, "snapshot 2", |s| !s.threads().is_empty());
    source2.watch(env.thread_id);
    settle(&mut source2, "events 2", |s| {
        s.events(env.thread_id).len() == 2
    });
    source2.apply(Msg::from(Update::Fatal(Error::UnsupportedVersion(
        "protocol 2.0".into(),
    ))));
    assert!(matches!(
        source2.connection(),
        Connection::VersionMismatch(_)
    ));
    assert!(source2.projects().is_empty());
    assert!(source2.threads().is_empty());
    assert!(source2.events(env.thread_id).is_empty());
}

#[gpui::test]
fn version_mismatch_leaves_the_thread_screen(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_component::init);
    let env = start();
    let port = env.port;
    let dir = tempfile::tempdir().expect("temp dir");
    let data = dir.path().to_path_buf();
    let (inject_tx, inject_rx) = flume::unbounded::<Msg>();
    let (app, cx) = cx.add_window_view(|window, cx| {
        let pins = data.clone();
        let inject_rx = inject_rx.clone();
        let connect = Box::new(move |_: &str| {
            let inner = RemoteSource::connect(NODE_NAME, options(port), &pins)?;
            let inner_inbox = inner.inbox().expect("remote inbox");
            let (tx, rx) = flume::unbounded();
            let tx2 = tx.clone();
            std::thread::spawn(move || {
                for m in inner_inbox.iter() {
                    if tx.send(m).is_err() {
                        break;
                    }
                }
            });
            let inject = inject_rx.clone();
            std::thread::spawn(move || {
                for m in inject.iter() {
                    if tx2.send(m).is_err() {
                        break;
                    }
                }
            });
            struct Wrapped {
                inner: RemoteSource,
                rx: flume::Receiver<Msg>,
            }
            impl ThreadSource for Wrapped {
                fn connection(&self) -> Connection {
                    self.inner.connection()
                }
                fn error(&self) -> Option<String> {
                    self.inner.error()
                }
                fn projects(&self) -> Vec<Project> {
                    self.inner.projects()
                }
                fn threads(&self) -> Vec<Thread> {
                    self.inner.threads()
                }
                fn events(&self, id: i64) -> &[skyelab_protocol::ThreadEvent] {
                    self.inner.events(id)
                }
                fn load_threads(&mut self) {
                    self.inner.load_threads();
                }
                fn watch(&mut self, id: i64) {
                    self.inner.watch(id);
                }
                fn send_message(&mut self, id: i64, text: &str) {
                    self.inner.send_message(id, text);
                }
                fn interrupt(&mut self, id: i64) {
                    self.inner.interrupt(id);
                }
                fn inbox(&self) -> Option<flume::Receiver<Msg>> {
                    Some(self.rx.clone())
                }
                fn apply(&mut self, msg: Msg) {
                    self.inner.apply(msg);
                }
            }
            Ok(Box::new(Wrapped { inner, rx }) as Box<dyn ThreadSource>)
        });
        MobileApp::new(data.clone(), connect, window, cx)
    });
    let env_input = app.read_with(cx, |a, _| a.env_input.clone());
    env_input.update_in(cx, |s, window, cx| s.set_value(NODE_NAME, window, cx));
    app.update_in(cx, |a, window, cx| a.connect(window, cx));
    let wait = |cx: &mut gpui::VisualTestContext, what: &str, done: &dyn Fn(&MobileApp) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            cx.run_until_parked();
            if app.read_with(cx, |a, _| done(a)) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    wait(cx, "threads", &|a| !a.threads().is_empty());
    app.update_in(cx, |a, window, cx| a.open_thread(env.thread_id, window, cx));
    wait(cx, "events", &|a| a.events().len() == 2);

    inject_tx
        .send(Msg::from(Update::Fatal(Error::UnsupportedVersion(
            "protocol 2.0".into(),
        ))))
        .expect("inject version mismatch");
    wait(cx, "version mismatch clear", &|a| {
        matches!(a.connection(), Some(Connection::VersionMismatch(_)))
    });
    app.read_with(cx, |a, _| {
        assert_eq!(a.screen, Screen::Threads, "leaves the thread screen");
        assert!(a.projects().is_empty());
        assert!(a.threads().is_empty());
        assert!(a.events().is_empty());
    });
}
