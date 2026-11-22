//! max_pending must bound whois calls actually running, not just futures still waiting for them.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::*;
use skyelab_daemon::Config;

#[test]
fn pending_cap_bounds_running_whois_after_gate_timeout() {
    // Every whois hangs 3 s; the gate gives up after 100 ms.
    let tailnet = FakeTailnet::new(Peer::Hang(Duration::from_secs(3)));
    let config = Config {
        max_pending: 1,
        gate_timeout: Duration::from_millis(100),
        ..test_config()
    };
    let daemon = start(tailnet.clone(), Arc::new(Counting::default()), config);
    let addr = daemon.local_addrs()[0];
    // Four connections in well under 3 s: every whois started is still running.
    for _ in 0..4 {
        assert!(try_connect(addr).is_err());
    }
    let running = tailnet.whois_calls.load(Ordering::SeqCst);
    assert!(
        running <= 1,
        "max_pending=1 but {running} whois calls are running at once"
    );
}
