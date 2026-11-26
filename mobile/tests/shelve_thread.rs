//! Acceptance: settling, snoozing (with presets) and waking/restoring threads from mobile work against a 1.7
//! daemon (reflected live via the shell stream) and are refused locally with the friendly "update SkyeLab on
//! <machine>" message on < 1.6 servers.

pub mod common;
#[path = "common/tls_ws.rs"]
mod tls_ws;

use std::net::TcpListener;
use std::time::{Duration, Instant};

use common::{NODE_NAME, Ran, options, start};
use gpui::TestAppContext;
use skyelab_mobile::{
    app::MobileApp,
    remote::RemoteSource,
    source::{Connection, ThreadSource},
    thread_rows::{now_ms, partition, snooze_badge},
};
use skyelab_protocol::{Call, Hello, Project, Reply, Request, ServerMessage, Thread, Version};
use tungstenite::Message;

fn make_thread(
    id: i64,
    status: &str,
    pinned: bool,
    updated_at: i64,
    settled_at: Option<i64>,
    snoozed_until: Option<i64>,
) -> Thread {
    Thread {
        id,
        project_id: 1,
        title: format!("t{id}"),
        account_id: "work".into(),
        account_label: Some("Work".into()),
        status: status.into(),
        pinned,
        preview: None,
        settled_at,
        snoozed_until,
        created_at: 0,
        updated_at,
        row: Default::default(),
    }
}

#[test]
fn partition_matches_desktop_shelf_precedence() {
    let starts = [1_000, 900, 300];
    let now = 2_000;
    let threads = vec![
        // Active today
        make_thread(1, "idle", false, 1_500, None, None),
        // Settled beats pinned
        make_thread(2, "idle", true, 1_900, Some(1_100), None),
        make_thread(3, "idle", false, 1_800, Some(1_400), None),
        // Snoozed (asleep) beats pinned; sorted soonest-waking first
        make_thread(4, "idle", true, 1_700, None, Some(5_000)),
        make_thread(5, "idle", false, 1_600, None, Some(3_000)),
        // Working beats a shelf
        make_thread(6, "running", false, 1_200, Some(1_000), Some(9_000)),
        // Woken snooze returns to active list
        make_thread(7, "idle", false, 1_400, None, Some(1_900)),
    ];
    let woke = threads[6].clone();
    let asleep = threads[4].clone();
    let lists = partition(threads, starts, now);

    let active_ids: Vec<(&str, Vec<i64>)> = lists
        .active
        .iter()
        .map(|(title, ts)| (*title, ts.iter().map(|t| t.id).collect()))
        .collect();
    assert_eq!(active_ids, [("Working", vec![6]), ("Today", vec![1, 7])]);
    assert_eq!(
        lists.snoozed.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![5, 4],
        "soonest waking first"
    );
    assert_eq!(
        lists.settled.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![3, 2],
        "most recently settled first"
    );
    assert_eq!(snooze_badge(&woke, now).as_deref(), Some("Woke"));
    assert!(
        snooze_badge(&asleep, now).is_some_and(|b| b.starts_with("Wakes in ")),
        "{:?}",
        snooze_badge(&asleep, now)
    );
}

#[gpui::test]
fn settle_snooze_and_wake_work_against_1_7_daemon(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_component::init);
    let env = start();
    let id = env.thread_id;
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

    wait(cx, "threads", &|a| {
        !a.threads().is_empty() && a.can_shelve()
    });

    // 1. Settle: daemon receives it and 1.7 shell stream moves the row into Settled.
    app.update(cx, |a, cx| a.settle_thread(id, cx));
    wait(cx, "settled live update", &|a| {
        a.threads()
            .iter()
            .any(|t| t.id == id && t.settled_at.is_some() && t.snoozed_until.is_none())
    });

    // 2. Snooze: daemon receives it and 1.7 shell stream moves the row into Snoozed.
    let until = now_ms() + 3_600_000;
    app.update(cx, |a, cx| a.snooze_thread(id, until, cx));
    wait(cx, "snoozed live update", &|a| {
        a.threads()
            .iter()
            .any(|t| t.id == id && t.settled_at.is_none() && t.snoozed_until == Some(until))
    });

    // 3. Wake / restore: daemon receives it and 1.7 shell stream returns the row to active.
    app.update(cx, |a, cx| a.wake_thread(id, cx));
    wait(cx, "woken live update", &|a| {
        a.threads()
            .iter()
            .any(|t| t.id == id && t.settled_at.is_none() && t.snoozed_until.is_none())
    });

    assert_eq!(
        env.ran(),
        vec![
            Ran::Shelve(id, true, None),
            Ran::Shelve(id, false, Some(until)),
            Ran::Shelve(id, false, None),
        ]
    );
}

fn server_1_5() -> (u16, flume::Receiver<String>) {
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
                Call::ShelveThread { .. } => "shelve_thread",
                Call::SubscribeShell => "subscribe_shell",
                _ => "other",
            };
            let _ = seen_tx.send(method.to_owned());
            let reply = match req.call {
                Call::Hello { .. } => ServerMessage::Response {
                    id: req.id,
                    result: Reply::Hello(Hello {
                        protocol: Version { major: 1, minor: 5 },
                        capabilities: vec![],
                        node_id: None,
                    }),
                },
                Call::ListProjects => ServerMessage::Response {
                    id: req.id,
                    result: Reply::Projects(vec![Project {
                        id: 1,
                        name: "old-box".into(),
                        path: "/src/old".into(),
                        created_at: 0,
                    }]),
                },
                Call::ListThreads { .. } => ServerMessage::Response {
                    id: req.id,
                    result: Reply::Threads(vec![make_thread(10, "idle", false, 500, None, None)]),
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

#[gpui::test]
fn server_below_1_6_refuses_shelve_locally_with_friendly_message(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_component::init);
    let (port, seen) = server_1_5();
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

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        cx.run_until_parked();
        if app.read_with(cx, |a, _| {
            a.connection() == Some(Connection::Connected) && !a.threads().is_empty()
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for 1.5 threads"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    app.read_with(cx, |a, _| {
        assert!(!a.can_shelve(), "1.5 does not speak shelve_thread");
        let reason = a.shelve_unavailable_reason();
        assert!(
            reason.contains(&format!("update SkyeLab on {NODE_NAME}")),
            "{reason}"
        );
    });

    // Attempting to settle fails locally with the friendly message and sends nothing on the wire.
    app.update(cx, |a, cx| a.settle_thread(10, cx));
    app.read_with(cx, |a, _| {
        let err = a.error.as_deref().expect("friendly error");
        assert!(
            err.contains(&format!("update SkyeLab on {NODE_NAME}")),
            "{err}"
        );
    });

    std::thread::sleep(Duration::from_millis(200));
    let calls: Vec<String> = seen.try_iter().collect();
    assert!(
        !calls.iter().any(|m| m == "shelve_thread"),
        "1.5 server must never receive shelve_thread: {calls:?}"
    );
    assert!(
        !calls.iter().any(|m| m == "subscribe_shell"),
        "1.5 server must never receive subscribe_shell: {calls:?}"
    );
}
