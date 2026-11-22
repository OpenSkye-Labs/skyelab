//! The security gate: only the user's own untagged devices get past whois, and nobody else is served anything.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use common::*;
use skyelab_daemon::testing::Peer;
use skyelab_protocol::{Call, Reply, ServerMessage, VERSION};

#[test]
fn own_device_can_connect_and_hello() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let backend = Arc::new(Counting::default());
    let daemon = start(tailnet.clone(), backend.clone(), test_config());
    let mut ws = connect(daemon.local_addrs()[0]);
    match hello_with(&mut ws, VERSION) {
        Some(ServerMessage::Response {
            id: 1,
            result: Reply::Hello(h),
        }) => assert_eq!(h.protocol, VERSION),
        other => panic!("{other:?}"),
    }
    send(&mut ws, 2, Call::ListProjects);
    assert!(matches!(
        recv(&mut ws),
        Some(ServerMessage::Response {
            id: 2,
            result: Reply::Projects(_)
        })
    ));
    assert_eq!(tailnet.whois_calls.load(Ordering::SeqCst), 1);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
}

/// Connecting must fail at the handshake, after whois ran, with no backend call.
fn assert_rejected(peer: Peer) {
    let tailnet = FakeTailnet::new(peer);
    let backend = Arc::new(Counting::default());
    let daemon = start(tailnet.clone(), backend.clone(), test_config());
    let started = Instant::now();
    let result = try_connect(daemon.local_addrs()[0]);
    assert!(result.is_err(), "handshake must not complete");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "rejection took {:?}",
        started.elapsed()
    );
    assert_eq!(
        tailnet.whois_calls.load(Ordering::SeqCst),
        1,
        "whois runs on every connection"
    );
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0, "nothing served");
}

#[test]
fn rejects_other_users_device() {
    assert_rejected(Peer::OtherUser);
}

#[test]
fn rejects_tagged_device() {
    assert_rejected(Peer::Tagged);
}

#[test]
fn rejects_on_whois_error() {
    assert_rejected(Peer::WhoisError);
}

#[test]
fn rejects_on_whois_timeout() {
    // gate_timeout is 500 ms; whois "hangs" for 10 s.
    assert_rejected(Peer::Hang(Duration::from_secs(10)));
}

#[test]
fn rejects_when_tailscale_goes_down_after_start() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let backend = Arc::new(Counting::default());
    let daemon = start(tailnet.clone(), backend.clone(), test_config());
    tailnet.down.store(true, Ordering::SeqCst);
    assert!(try_connect(daemon.local_addrs()[0]).is_err());
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn rejected_peer_does_not_block_own_device() {
    // A hanging whois for one peer must not stop the next connection being gated.
    let tailnet = FakeTailnet::new(Peer::Hang(Duration::from_secs(2)));
    let daemon = start(
        tailnet.clone(),
        Arc::new(Counting::default()),
        test_config(),
    );
    let addr = daemon.local_addrs()[0];
    let stuck = std::thread::spawn(move || try_connect(addr).is_err());
    std::thread::sleep(Duration::from_millis(50));
    *tailnet.peer.lock().expect("test setup") = Peer::Own;
    let mut ws = connect(addr);
    assert!(matches!(
        hello_with(&mut ws, VERSION),
        Some(ServerMessage::Response { .. })
    ));
    assert!(stuck.join().expect("test setup"));
}
