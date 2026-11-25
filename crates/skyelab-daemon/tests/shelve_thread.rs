//! `shelve_thread` (1.6) runs only after hello from an approved device; an unapproved or revoked device never
//! reaches the backend.

mod common;

use common::*;
use skyelab_daemon::testing::Peer;
use skyelab_protocol::{Call, ErrorCode};

fn settle() -> Call {
    Call::ShelveThread {
        thread_id: 1,
        settled: true,
        snoozed_until: None,
    }
}

#[test]
fn an_unapproved_device_cannot_shelve() {
    let backend = std::sync::Arc::new(Counting::default());
    let daemon = start(FakeTailnet::new(Peer::Own), backend.clone(), test_config());
    daemon.set_approved(vec![]);
    let mut ws = connect(daemon.local_addrs()[0]);
    send(&mut ws, 5, settle());
    assert_eq!(error_code(recv(&mut ws)), ErrorCode::HelloRequired);
    let mut ws = connect(daemon.local_addrs()[0]);
    assert_eq!(
        error_code(hello_key(&mut ws, Some(KEY))),
        ErrorCode::NotApproved
    );
    assert_eq!(backend.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn a_revoked_device_cannot_shelve() {
    let backend = std::sync::Arc::new(Counting::default());
    let daemon = start(FakeTailnet::new(Peer::Own), backend.clone(), test_config());
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    daemon.set_approved(vec![]);
    send(&mut ws, 5, settle());
    assert!(
        !matches!(
            recv(&mut ws),
            Some(skyelab_protocol::ServerMessage::Response { .. })
        ),
        "a revoked connection gets no answer"
    );
    assert_eq!(backend.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}
