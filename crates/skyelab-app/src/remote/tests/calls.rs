//! Remote agent calls end to end: a real daemon on 127.0.0.1, a ws client, the fake claude.

use super::*;

pub(super) struct Fixture {
    pub _dir: tempfile::TempDir,
    pub project_dir: PathBuf,
    pub account_dir: PathBuf,
    pub store: workspace::SharedStore,
    pub ws: Entity<Workspace>,
    pub remote: Entity<Remote>,
    /// The calls the daemon queued for the Workspace and it hasn't taken yet.
    pub queue: flume::Receiver<RemoteCall>,
}

/// A workspace with one project (bound to "acme" or not), sharing on, its remote calls wired.
pub(super) fn sharing_workspace(
    bound: bool,
    claude_bin: PathBuf,
    cx: &mut TestAppContext,
) -> (Fixture, &mut VisualTestContext) {
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
            provider: Default::default(),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("account");
    if bound {
        config
            .add_binding(Binding::new(project_dir.clone(), "acme".into()))
            .expect("bind");
    }
    let accounts_path = root.join("accounts.toml");
    config.save(&accounts_path).expect("save");
    // The phone is already allowed here, so these tests are about the calls.
    AppSettings {
        approved_devices: vec![skyelab_core::ApprovedDevice {
            node_id: "nPhone".into(),
            key_hash: skyelab_protocol::device_key_hash(PHONE_KEY),
            name: "phone.example.ts.net".into(),
            user: "me@example.com".into(),
        }],
        ..AppSettings::default()
    }
    .save(&root.join("settings.toml"))
    .expect("settings");

    // One store for the app and the daemon, with events pushed, as in main.
    let slot = DaemonSlot::default();
    let mut store = Store::open_in_memory().expect("store");
    push_events(&mut store, slot.clone());
    let store = Arc::new(Mutex::new(store));
    let (h, calls) = host(&root, Arc::default(), store.clone(), slot);
    let queue = calls.clone();
    let remote = cx.new(|cx| Remote::new(h, cx));
    let backend = workspace::Backend {
        store: store.clone(),
        claude_bin,
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
            queue,
        },
        cx,
    )
}

pub(super) fn connect_hello(f: &Fixture, cx: &mut VisualTestContext) -> Client {
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
            device_key: Some(PHONE_KEY.into()),
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

pub(super) fn error_of(msg: ServerMessage) -> skyelab_protocol::RpcError {
    match msg {
        ServerMessage::Error { error, .. } => error,
        other => panic!("expected an error, got {other:?}"),
    }
}

#[gpui::test]
fn remote_start_runs_under_the_bound_account_shows_locally_and_pushes_events(
    cx: &mut TestAppContext,
) {
    let (f, cx) = sharing_workspace(true, fake_bin(), cx);
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
    let (f, cx) = sharing_workspace(false, fake_bin(), cx);
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
    let (f, cx) = sharing_workspace(true, fake_bin(), cx);
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

#[gpui::test]
fn remote_start_whose_agent_fails_to_launch_is_an_error(cx: &mut TestAppContext) {
    let (f, cx) = sharing_workspace(true, PathBuf::from("/nonexistent/claude"), cx);
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
    assert_eq!(error.message, "the agent didn't start");
    assert!(
        !error.message.contains("/nonexistent"),
        "no local paths go to the client"
    );
}
