//! Failed connections are retried with a growing delay, not in a tight loop.

use std::sync::Arc;
use std::time::Duration;

use super::*;

#[test]
fn rejected_connections_back_off() {
    let tailnet = Arc::new(FakeTailnet::default());
    tailnet.set_peer(Peer::WhoisError);
    let env = Env::new();
    let daemon = start_daemon(tailnet.clone(), env.backend(), 0);
    let opts = Options {
        backoff_min: Duration::from_millis(250),
        backoff_max: Duration::from_secs(30),
        ..options(port(&daemon))
    };
    let client = Client::connect(HOST, opts).expect("client");
    // Each refused attempt reports one Disconnected. Timed here, not by the daemon's whois calls, which would
    // also count strays (another test's client retrying a port this daemon now holds).
    let refused = || loop {
        if matches!(next(&client), Update::Disconnected(_)) {
            return std::time::Instant::now();
        }
    };
    let first = refused();
    refused();
    let third = refused();
    // Delays of 0.25 then 0.5 s put the third attempt at least 0.75 s after the first; a missing delay would
    // make it milliseconds. A slow machine only stretches the gap, so this lower bound can't flake.
    assert!(
        third - first >= Duration::from_millis(700),
        "three attempts in {:?}",
        third - first
    );
    drop(client);
}
