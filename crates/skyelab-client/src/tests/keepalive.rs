//! Pings keep an idle connection open past the daemon's idle timeout.

use std::sync::Arc;
use std::time::Duration;

use skyelab_daemon::{Config, Daemon};

use super::*;

#[test]
fn pings_keep_the_connection_alive() {
    let env = Env::new();
    let config = Config {
        // 20 pings per idle window: one stalled for seconds on a loaded machine must not drop the connection.
        idle_timeout: Duration::from_secs(4),
        ..daemon_config(0)
    };
    let daemon =
        Daemon::start(config, Arc::new(FakeTailnet::default()), env.backend()).expect("daemon");
    let opts = Options {
        ping_interval: Duration::from_millis(200),
        ..options(port(&daemon))
    };
    let client = Client::connect(HOST, opts).expect("client");
    connected(&client);
    quiet(&client, Duration::from_secs(6));
    assert!(client.hello().is_some());
}
