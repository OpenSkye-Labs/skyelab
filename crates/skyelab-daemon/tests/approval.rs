//! Device approval: only a device key the user approved, on the node it was approved for, gets past hello.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use common::*;
use skyelab_daemon::Approval;
use skyelab_daemon::testing::{ME, Peer};
use skyelab_protocol::{Call, ErrorCode, Reply, ServerMessage, device_code, device_key_hash};

const OTHER_KEY: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn approval(node: &str, key: &str) -> Approval {
    Approval {
        node_id: node.into(),
        key_hash: device_key_hash(key),
    }
}

/// Says hello with `key`; the error code, or `None` if hello passed.
fn hello_code(ws: &mut Client, key: Option<&str>) -> Option<ErrorCode> {
    match hello_key(ws, key) {
        Some(ServerMessage::Response {
            result: Reply::Hello(_),
            ..
        }) => None,
        Some(ServerMessage::Error { error, .. }) => Some(error.code),
        other => panic!("unexpected answer to hello: {other:?}"),
    }
}

/// The server closed: the next read ends instead of answering.
fn assert_closed(ws: &mut Client) {
    assert!(recv(ws).is_none(), "the connection must be closed");
}

#[test]
fn unknown_device_is_not_approved_and_asks() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let backend = Arc::new(Counting::default());
    let daemon = start(tailnet, backend.clone(), test_config());
    daemon.set_approved(vec![]);
    let mut ws = connect(daemon.local_addrs()[0]);
    assert_eq!(hello_code(&mut ws, Some(KEY)), Some(ErrorCode::NotApproved));
    assert_closed(&mut ws);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0, "nothing served");
    let requests = daemon.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    let r = &requests[0];
    assert_eq!(r.node_id, "nPhone");
    assert_eq!(r.device, "phone.example.ts.net");
    assert_eq!(r.user, format!("user{ME}@example.com"));
    assert_eq!(r.key_hash, device_key_hash(KEY));
    assert_eq!(r.code, device_code(&device_key_hash(KEY)));
    assert_eq!(
        r.code.len(),
        19,
        "XXXX-XXXX-XXXX-XXXX (64 bits): {}",
        r.code
    );
    // A second try is the same request, not a new one.
    let mut ws = connect(daemon.local_addrs()[0]);
    assert_eq!(hello_code(&mut ws, Some(KEY)), Some(ErrorCode::NotApproved));
    assert_eq!(daemon.requests().len(), 1);
}

#[test]
fn approved_device_is_served() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let backend = Arc::new(Counting::default());
    let daemon = start(tailnet, backend.clone(), test_config());
    daemon.set_approved(vec![approval("nPhone", KEY)]);
    let mut ws = connect(daemon.local_addrs()[0]);
    assert_eq!(hello_code(&mut ws, Some(KEY)), None);
    send(&mut ws, 2, Call::ListProjects);
    assert!(matches!(
        recv(&mut ws),
        Some(ServerMessage::Response {
            result: Reply::Projects(_),
            ..
        })
    ));
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    assert!(daemon.requests().is_empty());
    let peers = daemon.peers();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].node_id, "nPhone");
}

#[test]
fn approval_is_for_the_key_and_the_node_together() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let backend = Arc::new(Counting::default());
    let daemon = start(tailnet.clone(), backend.clone(), test_config());
    daemon.set_approved(vec![approval("nPhone", KEY)]);
    // Another process on the approved node, with its own key.
    let mut ws = connect(daemon.local_addrs()[0]);
    assert_eq!(
        hello_code(&mut ws, Some(OTHER_KEY)),
        Some(ErrorCode::NotApproved)
    );
    // The approved key, replayed from another of the user's nodes.
    *tailnet.peer.lock().expect("test setup") = Peer::OwnNode("nTablet");
    let mut ws = connect(daemon.local_addrs()[0]);
    assert_eq!(hello_code(&mut ws, Some(KEY)), Some(ErrorCode::NotApproved));
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    assert_eq!(daemon.requests().len(), 2, "each is its own request");
}

#[test]
fn revoking_closes_open_connections_at_once() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let daemon = start(tailnet, Arc::new(Counting::default()), test_config());
    daemon.set_approved(vec![approval("nPhone", KEY)]);
    let mut ws = connect(daemon.local_addrs()[0]);
    assert_eq!(hello_code(&mut ws, Some(KEY)), None);
    let revoked = Instant::now();
    daemon.set_approved(vec![]);
    assert_closed(&mut ws);
    assert!(
        revoked.elapsed() < Duration::from_secs(1),
        "closed after {:?}",
        revoked.elapsed()
    );
    let mut ws = connect(daemon.local_addrs()[0]);
    assert_eq!(hello_code(&mut ws, Some(KEY)), Some(ErrorCode::NotApproved));
}

#[test]
fn revoking_one_device_keeps_the_others() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let daemon = start(tailnet, Arc::new(Counting::default()), test_config());
    daemon.set_approved(vec![approval("nPhone", KEY), approval("nPhone", OTHER_KEY)]);
    let mut kept = connect(daemon.local_addrs()[0]);
    assert_eq!(hello_code(&mut kept, Some(KEY)), None);
    let mut gone = connect(daemon.local_addrs()[0]);
    assert_eq!(hello_code(&mut gone, Some(OTHER_KEY)), None);
    daemon.set_approved(vec![approval("nPhone", KEY)]);
    assert_closed(&mut gone);
    send(&mut kept, 2, Call::ListProjects);
    assert!(matches!(
        recv(&mut kept),
        Some(ServerMessage::Response { id: 2, .. })
    ));
}

#[test]
fn a_denied_device_is_told_so_and_not_asked_again() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let daemon = start(tailnet, Arc::new(Counting::default()), test_config());
    daemon.set_approved(vec![]);
    let mut ws = connect(daemon.local_addrs()[0]);
    assert_eq!(hello_code(&mut ws, Some(KEY)), Some(ErrorCode::NotApproved));
    daemon.deny("nPhone", &device_key_hash(KEY));
    assert!(daemon.requests().is_empty());
    let mut ws = connect(daemon.local_addrs()[0]);
    assert_eq!(hello_code(&mut ws, Some(KEY)), Some(ErrorCode::Denied));
    assert_closed(&mut ws);
    assert!(
        daemon.requests().is_empty(),
        "a denied device isn't asked about again"
    );
}

#[test]
fn a_hello_without_a_valid_key_is_refused_and_asks_nothing() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let backend = Arc::new(Counting::default());
    let daemon = start(tailnet, backend.clone(), test_config());
    daemon.set_approved(vec![approval("nPhone", KEY)]);
    for key in [None, Some("short"), Some(&KEY.to_uppercase()[..])] {
        let mut ws = connect(daemon.local_addrs()[0]);
        assert_eq!(
            hello_code(&mut ws, key),
            Some(ErrorCode::NotApproved),
            "{key:?}"
        );
    }
    assert!(daemon.requests().is_empty());
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn one_node_cannot_crowd_out_another_devices_request() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let daemon = start(
        tailnet.clone(),
        Arc::new(Counting::default()),
        test_config(),
    );
    daemon.set_approved(vec![]);
    for n in 0..40u32 {
        let mut ws = connect(daemon.local_addrs()[0]);
        let _ = hello_key(&mut ws, Some(&format!("{n:064x}")));
    }
    let from_phone = daemon.requests();
    assert_eq!(from_phone.len(), 2, "two per node, the newest");
    assert_eq!(
        from_phone[1].key_hash,
        device_key_hash(&format!("{:064x}", 39))
    );
    *tailnet.peer.lock().expect("test setup") = Peer::OwnNode("nTablet");
    let mut ws = connect(daemon.local_addrs()[0]);
    let _ = hello_key(&mut ws, Some(KEY));
    assert!(
        daemon.requests().iter().any(|r| r.node_id == "nTablet"),
        "the tablet is still asked about"
    );
}

#[test]
fn only_approved_connections_are_listed() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let daemon = start(tailnet, Arc::new(Counting::default()), test_config());
    daemon.set_approved(vec![]);
    let _waiting = connect(daemon.local_addrs()[0]); // no hello yet
    std::thread::sleep(Duration::from_millis(100));
    assert!(daemon.peers().is_empty(), "{:?}", daemon.peers());
}

#[test]
fn requests_are_capped() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let daemon = start(
        tailnet.clone(),
        Arc::new(Counting::default()),
        test_config(),
    );
    daemon.set_approved(vec![]);
    // Two per node, so it takes many nodes to fill the list.
    for node in 0..20usize {
        let id: &'static str = Box::leak(format!("n{node}").into_boxed_str());
        *tailnet.peer.lock().expect("test setup") = Peer::OwnNode(id);
        let mut ws = connect(daemon.local_addrs()[0]);
        assert_eq!(hello_code(&mut ws, Some(KEY)), Some(ErrorCode::NotApproved));
    }
    assert_eq!(daemon.requests().len(), 16);
}
