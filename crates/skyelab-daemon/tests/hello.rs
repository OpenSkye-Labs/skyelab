//! Version negotiation and hello-first.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use common::*;
use skyelab_protocol::{Call, ErrorCode, VERSION, Version};

#[test]
fn unknown_major_version_rejected() {
    let backend = Arc::new(Counting::default());
    let daemon = start(FakeTailnet::new(Peer::Own), backend.clone(), test_config());
    let mut ws = connect(daemon.local_addrs()[0]);
    let reply = hello_with(
        &mut ws,
        Version {
            major: VERSION.major + 1,
            minor: 0,
        },
    );
    assert_eq!(error_code(reply), ErrorCode::UnsupportedVersion);
    assert!(
        recv(&mut ws).is_none(),
        "connection closes after a version mismatch"
    );
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn request_before_hello_rejected() {
    let backend = Arc::new(Counting::default());
    let daemon = start(FakeTailnet::new(Peer::Own), backend.clone(), test_config());
    let mut ws = connect(daemon.local_addrs()[0]);
    send(&mut ws, 5, Call::ListProjects);
    assert_eq!(error_code(recv(&mut ws)), ErrorCode::HelloRequired);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn second_hello_is_bad_request() {
    let daemon = start(
        FakeTailnet::new(Peer::Own),
        Arc::new(Counting::default()),
        test_config(),
    );
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    assert_eq!(
        error_code(hello_with(&mut ws, VERSION)),
        ErrorCode::BadRequest
    );
}

#[test]
fn garbage_is_bad_request() {
    let daemon = start(
        FakeTailnet::new(Peer::Own),
        Arc::new(Counting::default()),
        test_config(),
    );
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    ws.send(tungstenite::Message::text("{not json")).unwrap();
    assert_eq!(error_code(recv(&mut ws)), ErrorCode::BadRequest);
}
