//! Acceptance: `RemoteSource` over `skyelab-client` against an in-process `skyelab-daemon` with a fake tailnet.

pub mod common;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use common::{Env, NODE_NAME, Peer, Ran, options, start};
use skyelab_client::{Error, Update};
use skyelab_mobile::{
    remote::RemoteSource,
    source::{Connection, Msg, ThreadSource},
};

/// Applies whatever arrives until `done` holds, failing after 5 s.
fn settle(source: &mut RemoteSource, what: &str, done: impl Fn(&RemoteSource) -> bool) {
    let inbox = source.inbox().expect("a remote source has an inbox");
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

fn connect(env: &Env) -> RemoteSource {
    RemoteSource::connect(NODE_NAME, options(env.port)).expect("client thread")
}

#[test]
fn connects_then_lists_projects_and_threads() {
    let env = start();
    let mut source = connect(&env);
    assert_eq!(source.connection(), Connection::Connecting);
    settle(&mut source, "projects", |s| !s.projects().is_empty());
    assert_eq!(source.connection(), Connection::Connected);
    assert_eq!(source.projects()[0].id, env.project_id);

    source.load_threads(env.project_id);
    settle(&mut source, "threads", |s| {
        !s.threads(env.project_id).is_empty()
    });
    assert_eq!(source.threads(env.project_id)[0].title, "fix it");
}

#[test]
fn a_watched_thread_gets_stored_then_live_events() {
    let env = start();
    let mut source = connect(&env);
    settle(&mut source, "connected", |s| {
        s.connection() == Connection::Connected
    });
    source.watch(env.thread_id);
    settle(&mut source, "stored events", |s| {
        s.events(env.thread_id).len() == 2
    });

    let live = env.push(
        "agent",
        r#"{"Assistant":{"content":[{"type":"text","text":"done"}]}}"#,
    );
    settle(&mut source, "the live event", |s| {
        s.events(env.thread_id).len() == 3
    });
    let seqs: Vec<i64> = source.events(env.thread_id).iter().map(|e| e.seq).collect();
    assert_eq!(seqs, vec![1, 2, live.seq]);
}

#[test]
fn send_and_stop_reach_the_daemon_and_failures_show() {
    let env = start();
    let mut source = connect(&env);
    settle(&mut source, "connected", |s| {
        s.connection() == Connection::Connected
    });

    source.send_message(env.thread_id, "run the tests");
    source.interrupt(env.thread_id);
    // The daemon serves requests concurrently, so they may land in either order.
    let mut want = vec![
        Ran::Send(env.thread_id, "run the tests".into()),
        Ran::Interrupt(env.thread_id),
    ];
    want.sort();
    let sorted = || {
        let mut calls = env.ran();
        calls.sort();
        calls
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while sorted() != want {
        assert!(Instant::now() < deadline, "daemon saw {:?}", env.ran());
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(source.error(), None);

    env.agents.fail.store(true, Ordering::SeqCst);
    source.send_message(env.thread_id, "again");
    settle(&mut source, "the error", |s| s.error().is_some());
    let error = source.error().expect("an error");
    assert!(error.contains("not wired up"), "{error}");
}

#[test]
fn a_device_the_daemon_rejects_shows_refused() {
    let env = start();
    env.tailnet.set_peer(Peer::Tagged);
    let mut source = connect(&env);
    settle(&mut source, "refused", |s| {
        s.connection() == Connection::Refused
    });
}

#[test]
fn a_lost_daemon_shows_reconnecting() {
    let mut env = start();
    let mut source = connect(&env);
    settle(&mut source, "connected", |s| {
        s.connection() == Connection::Connected
    });
    drop(env.daemon.take());
    settle(&mut source, "reconnecting", |s| {
        matches!(s.connection(), Connection::Reconnecting(_))
    });
}

#[test]
fn an_unsupported_protocol_shows_version_mismatch() {
    let env = start();
    let mut source = connect(&env);
    source.apply(Msg::from(Update::Fatal(Error::UnsupportedVersion(
        "the environment answered with protocol 9.0".into(),
    ))));
    match source.connection() {
        Connection::VersionMismatch(why) => assert!(why.contains("9.0"), "{why}"),
        other => panic!("expected VersionMismatch, got {other:?}"),
    }
}

/// Without the `demo` feature, the app dials the real client for every name.
#[cfg(not(feature = "demo"))]
#[test]
fn shipping_build_dials_the_real_client() {
    let source = skyelab_mobile::app::source_for("laptop.tail1234.ts.net").expect("source");
    assert_eq!(source.connection(), Connection::Connecting);
    assert!(source.inbox().is_some(), "the fake has no inbox");
}
