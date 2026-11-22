//! Pings keep an idle connection open past the daemon's idle timeout.

use std::sync::Arc;
use std::time::Duration;

use skyelab_daemon::Daemon;

use super::*;

#[test]
fn pings_keep_the_connection_alive() {
    let env = Env::new();
    let config = Config {
        idle_timeout: Duration::from_secs(1),
        ..daemon_config(0)
    };
    let daemon =
        Daemon::start(config, Arc::new(FakeTailnet::default()), env.backend()).expect("daemon");
    let opts = Options {
        ping_interval: Duration::from_millis(200),
        ..options(port(&daemon))
    };
    let client = Client::connect("127.0.0.1", opts).expect("client");
    connected(&client);
    quiet(&client, Duration::from_secs(3));
    assert!(client.hello().is_some());
}
