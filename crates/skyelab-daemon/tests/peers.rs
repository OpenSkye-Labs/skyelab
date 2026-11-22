//! Who is connected, and kicking one of them.

mod common;

use std::sync::Arc;

use common::*;
use skyelab_daemon::testing::{ME, Peer};

#[test]
fn lists_gated_peers_and_disconnect_closes_only_that_socket() {
    let daemon = start(
        FakeTailnet::new(Peer::Own),
        Arc::new(Counting::default()),
        test_config(),
    );
    let addr = daemon.local_addrs()[0];
    assert!(daemon.peers().is_empty());
    // The hello reply comes from the serve loop, which starts after the peer is registered.
    let mut first = connect_hello(addr);
    let mut second = connect_hello(addr);
    let peers = daemon.peers();
    assert_eq!(peers.len(), 2, "{peers:?}");
    assert_eq!(
        peers[0].device, "phone.example.ts.net",
        "trailing dot trimmed"
    );
    assert_eq!(peers[0].user, format!("user{ME}@example.com"));
    assert_ne!(peers[0].id, peers[1].id);

    assert!(daemon.disconnect(peers[0].id));
    assert_eq!(recv(&mut first), None, "the kicked socket is closed");
    // The other connection still works.
    send(&mut second, 2, skyelab_protocol::Call::ListProjects);
    assert!(matches!(
        recv(&mut second),
        Some(skyelab_protocol::ServerMessage::Response { id: 2, .. })
    ));
    // Its registry entry goes once the serve loop ends.
    for _ in 0..100 {
        if daemon.peers().len() == 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(daemon.peers(), vec![peers[1].clone()]);
    assert!(!daemon.disconnect(peers[0].id), "already gone");
}

#[test]
fn rejected_peers_are_never_listed() {
    let daemon = start(
        FakeTailnet::new(Peer::OtherUser),
        Arc::new(Counting::default()),
        test_config(),
    );
    assert!(try_connect(daemon.local_addrs()[0]).is_err());
    assert!(daemon.peers().is_empty());
}
