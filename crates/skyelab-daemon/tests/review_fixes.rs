mod common;
use common::*;
use skyelab_daemon::{Config, Daemon};
use skyelab_protocol::Call;
use std::sync::Arc;

#[test]
fn start_rejects_zero_limits() {
    for config in [
        Config {
            max_in_flight: 0,
            ..test_config()
        },
        Config {
            max_pending: 0,
            ..test_config()
        },
        Config {
            max_message_bytes: 0,
            ..test_config()
        },
    ] {
        assert!(
            Daemon::start(
                config,
                FakeTailnet::new(Peer::Own),
                Arc::new(Counting::default())
            )
            .is_err()
        );
    }
}

#[test]
fn start_rejects_ipv4_mapped_wildcard() {
    let tailnet = Arc::new(FakeTailnet {
        ips: vec!["::ffff:0.0.0.0".parse().expect("test setup")],
        ..Arc::into_inner(FakeTailnet::new(Peer::Own)).expect("test setup")
    });
    assert!(Daemon::start(test_config(), tailnet, Arc::new(Counting::default())).is_err());
}

#[test]
fn lagging_client_is_disconnected_not_silently_skipped() {
    let d = start(
        FakeTailnet::new(Peer::Own),
        Arc::new(Counting::default()),
        test_config(),
    );
    let mut ws = connect_hello(d.local_addrs()[0]);
    // More than the broadcast buffer, pushed before the connection task can drain any of it.
    for seq in 1..=5000 {
        d.push_event(skyelab_protocol::ThreadEvent {
            thread_id: 1,
            seq,
            kind: "k".into(),
            payload_json: "{}".into(),
            created_at: 0,
        });
    }
    let mut got = 0;
    while recv(&mut ws).is_some() {
        got += 1;
    }
    assert!(
        got < 5000,
        "a lagging client must be closed, not handed a silent gap"
    );
}

#[test]
fn thread_events_since_pages() {
    use skyelab_daemon::MAX_EVENTS_PER_REPLY;
    use skyelab_protocol::{Reply, ServerMessage};
    let mut store = skyelab_store::Store::open_in_memory().expect("test setup");
    let p = store
        .create_project("app", std::path::Path::new("/src/app"))
        .expect("test setup");
    let t = store
        .create_thread(&skyelab_store::NewThread {
            project_id: p.id,
            title: "t",
            cwd: std::path::Path::new("/src/app"),
            account_id: "work",
            status: "idle",
        })
        .expect("test setup");
    for _ in 0..MAX_EVENTS_PER_REPLY + 3 {
        store.append_event(t.id, "k", "{}").expect("test setup");
    }
    let d = start(
        FakeTailnet::new(Peer::Own),
        store_backend(store),
        test_config(),
    );
    let mut ws = connect_hello(d.local_addrs()[0]);
    let mut seen = Vec::new();
    loop {
        send(
            &mut ws,
            2,
            Call::ThreadEventsSince {
                thread_id: t.id,
                seq: seen.last().copied().unwrap_or(0),
            },
        );
        match recv(&mut ws) {
            Some(ServerMessage::Response {
                result: Reply::Events(e),
                ..
            }) if e.is_empty() => break,
            Some(ServerMessage::Response {
                result: Reply::Events(e),
                ..
            }) => {
                assert!(e.len() <= MAX_EVENTS_PER_REPLY);
                seen.extend(e.iter().map(|e| e.seq));
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(
        seen,
        (1..=(MAX_EVENTS_PER_REPLY as i64 + 3)).collect::<Vec<_>>()
    );
}
