//! Connection states of a remote environment, and that waiting on one never blocks the UI thread.

use super::env_harness::*;
use super::*;
use gpui::TestAppContext;
use skyelab_daemon::{Backend as DaemonBackend, Config, Daemon};
use skyelab_protocol::{
    Hello, Project, Reply, RpcError, ServerMessage, Thread, ThreadEvent, Version,
};
use std::sync::atomic::Ordering;
use std::time::Duration;

#[gpui::test]
fn remote_connecting_connected_reconnecting_recovered(cx: &mut TestAppContext) {
    let mut remote = RemoteSide::new(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    switch(&ws, Some("127.0.0.1"), cx);
    assert_eq!(state(&ws, cx), "Connecting…", "before hello");
    wait_until(cx, "connected", |cx| state(&ws, cx) == "Connected");

    remote.stop();
    wait_until(cx, "reconnecting", |cx| state(&ws, cx).starts_with("Reconnecting"));
    let badge = ws.read_with(cx, |ws, cx| ws.env_badge_text(cx)).expect("badge");
    assert!(badge.contains("Reconnecting"), "{badge}");

    remote.start();
    wait_until(cx, "recovered", |cx| state(&ws, cx) == "Connected");
    // And it lists again after the reconnect.
    wait_until(cx, "projects", |cx| sidebar(&ws, cx)[0].1 == vec!["app".to_owned()]);
}

#[gpui::test]
fn remote_refused_by_whois_says_so(cx: &mut TestAppContext) {
    let tailnet = FakeTailnet::default();
    tailnet.refuse.store(true, Ordering::SeqCst);
    let remote = RemoteSide::new(tailnet);
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    switch(&ws, Some("127.0.0.1"), cx);
    wait_until(cx, "refused", |cx| {
        state(&ws, cx) == "That machine didn't accept this device"
    });
    // Once it accepts us (the user fixed the ACL), the retry connects.
    remote.tailnet.refuse.store(false, Ordering::SeqCst);
    wait_until(cx, "connected", |cx| state(&ws, cx) == "Connected");
}

/// A server that answers hello with protocol 99.
fn newer_server() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let Ok(mut ws) = tungstenite::accept(stream) else {
                continue;
            };
            let _ = ws.read(); // the hello
            let hello = ServerMessage::Response {
                id: 0,
                result: Reply::Hello(Hello {
                    protocol: Version {
                        major: 99,
                        minor: 0,
                    },
                    capabilities: vec![],
                }),
            };
            let _ = ws.send(tungstenite::Message::text(hello.to_json()));
        }
    });
    port
}

#[gpui::test]
fn remote_version_mismatch_is_fatal_and_clear(cx: &mut TestAppContext) {
    let port = newer_server();
    let options = skyelab_client::Options {
        port,
        backoff_min: Duration::from_millis(50),
        ..skyelab_client::Options::default()
    }
    .allow_loopback();
    let (ws, _local, cx) = local_workspace(Arc::default(), None, options, cx);
    switch(&ws, Some("127.0.0.1"), cx);
    wait_until(cx, "mismatch", |cx| state(&ws, cx).starts_with("Version mismatch"));
    let label = state(&ws, cx);
    assert!(label.contains("update SkyeLab"), "{label}");
    // Fatal: it stays there.
    for _ in 0..20 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(state(&ws, cx).starts_with("Version mismatch"));
}

/// Serves everything from `inner`, except that list_projects waits for `release`.
struct Stall {
    inner: Arc<dyn DaemonBackend>,
    release: flume::Receiver<()>,
}

impl DaemonBackend for Stall {
    fn list_projects(&self) -> Result<Vec<Project>, RpcError> {
        let _ = self.release.recv();
        self.inner.list_projects()
    }
    fn list_threads(&self, project_id: i64) -> Result<Vec<Thread>, RpcError> {
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

#[gpui::test]
fn remote_waiting_on_a_reply_never_blocks_the_ui(cx: &mut TestAppContext) {
    let mut remote = RemoteSide::new(FakeTailnet::default());
    remote.stop();
    let (release_tx, release) = flume::unbounded();
    let inner: Arc<dyn DaemonBackend> = Arc::new(skyelab_daemon::StoreBackend::shared(
        remote.store.clone(),
        skyelab_daemon::NotWired,
    ));
    let config = Config {
        port: remote.port,
        ..Config::default()
    };
    let _daemon = Daemon::start(
        config,
        remote.tailnet.clone(),
        Arc::new(Stall { inner, release }),
    )
    .expect("daemon");
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    switch(&ws, Some("127.0.0.1"), cx);
    wait_until(cx, "connected", |cx| state(&ws, cx) == "Connected");

    // list_projects is in flight and stuck. The UI still runs actions and renders.
    let open = |cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| ws.env_picker().expect("picker").read(cx).open)
    };
    let before = open(cx);
    cx.dispatch_action(env_picker::ToggleEnvironments);
    cx.run_until_parked();
    assert_ne!(open(cx), before, "the action ran while a reply was pending");
    assert!(sidebar(&ws, cx)[0].1.is_empty(), "the reply hasn't come");

    release_tx.send(()).expect("release");
    wait_until(cx, "projects", |cx| sidebar(&ws, cx)[0].1 == vec!["app".to_owned()]);
}
