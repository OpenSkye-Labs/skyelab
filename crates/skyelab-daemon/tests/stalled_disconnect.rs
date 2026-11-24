//! Disconnect closes a connection at once, even while the daemon is stuck writing to a client that stopped reading.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use skyelab_daemon::testing::Peer;

#[test]
fn disconnect_closes_a_stalled_writer() {
    let daemon = start(
        FakeTailnet::new(Peer::Own),
        Arc::new(Counting::default()),
        test_config(), // idle_timeout is 300 s, the bound the stalled write waited for
    );
    let _ws = connect_hello(daemon.local_addrs()[0]); // never read again
    let id = daemon.peers()[0].id;
    // Far more than the socket buffers hold, and fewer than the broadcast buffer, so it stalls rather than lags.
    let payload = format!("\"{}\"", "x".repeat(512 << 10));
    for seq in 1..=64 {
        daemon.push_event(skyelab_protocol::ThreadEvent {
            thread_id: 1,
            seq,
            kind: "k".into(),
            payload_json: payload.clone(),
            created_at: 0,
        });
    }
    std::thread::sleep(Duration::from_millis(500)); // the serve loop is now blocked in a send

    assert!(daemon.disconnect(id));
    let asked = Instant::now();
    while !daemon.peers().is_empty() {
        assert!(
            asked.elapsed() < Duration::from_secs(30), // far under the 300 s write bound, far over a slow machine
            "still connected: Disconnect waited for the stalled write"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
