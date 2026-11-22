//! A web page open on one of the user's own devices passes whois, so browser handshakes are refused.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use common::*;

#[test]
fn rejects_browser_origin() {
    let backend = Arc::new(Counting::default());
    let daemon = start(FakeTailnet::new(Peer::Own), backend.clone(), test_config());
    let err = try_connect_with_origin(daemon.local_addrs()[0], "https://evil.example").unwrap_err();
    match err {
        tungstenite::Error::Http(resp) => assert_eq!(resp.status(), 403),
        other => panic!("expected HTTP 403, got {other:?}"),
    }
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
}
