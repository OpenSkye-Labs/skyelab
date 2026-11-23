//! Acceptance: a refusal after a trusted connection leaves nothing of that machine on screen.

pub mod common;

use std::time::{Duration, Instant};

use common::{NODE_NAME, Peer, options, start};
use gpui::TestAppContext;
use skyelab_mobile::{
    app::{MobileApp, Screen},
    remote::RemoteSource,
    source::{Connection, ThreadSource},
};

#[gpui::test]
fn a_refusal_empties_the_lists_and_leaves_the_thread(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_component::init);
    let mut env = start();
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
    wait(cx, "threads", &|a| !a.threads().is_empty());
    app.update_in(cx, |a, window, cx| a.open_thread(env.thread_id, window, cx));
    wait(cx, "stored events", &|a| a.events().len() == 2);

    env.stop();
    env.tailnet.set_peer(Peer::Tagged);
    env.serve();
    wait(cx, "the refusal", &|a| {
        a.connection() == Some(Connection::Refused)
    });
    app.read_with(cx, |a, _| {
        assert_eq!(a.screen, Screen::Threads, "back off the thread");
        assert!(a.projects().is_empty(), "no projects left");
        assert!(a.events().is_empty());
    });
    // The threads are gone too.
    app.read_with(cx, |a, _| {
        assert!(a.threads().is_empty(), "no threads left")
    });
}

#[test]
fn a_certificate_failure_clears_the_lists_too() {
    use skyelab_client::{Disconnect, DisconnectKind, Update};
    use skyelab_mobile::source::Msg;
    let env = start();
    let mut source =
        RemoteSource::connect(NODE_NAME, options(env.port), env._dir.path()).expect("client");
    let inbox = source.inbox().expect("inbox");
    let deadline = Instant::now() + Duration::from_secs(5);
    while source.projects().is_empty() {
        let left = deadline.saturating_duration_since(Instant::now());
        source.apply(inbox.recv_timeout(left).expect("projects within 5 s"));
    }
    source.apply(Msg::from(Update::Disconnected(Disconnect {
        kind: DisconnectKind::Certificate,
        message: "invalid peer certificate: NotValidForName".into(),
    })));
    assert!(
        source.connection().is_untrusted(),
        "{:?}",
        source.connection()
    );
    assert!(source.projects().is_empty());
}
