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
    std::thread::sleep(Duration::from_secs(2));
    // Each refused attempt reports one Disconnected. Counted here, not by the daemon's whois calls, which would
    // also count strays (another test's client retrying a port this daemon now holds).
    let attempts = client
        .updates()
        .try_iter()
        .filter(|u| matches!(u, Update::Disconnected(_)))
        .count();
    // 0, 0.25, 0.75, 1.75 s: four attempts; a missing delay would make hundreds.
    assert!((2..=5).contains(&attempts), "{attempts} attempts in 2 s");
    drop(client);
}
