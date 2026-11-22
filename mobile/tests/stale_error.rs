pub mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{Env, NODE_NAME, RecordingAgents, options};
use skyelab_daemon::{Config, Daemon, StoreBackend};
use skyelab_mobile::{
    remote::RemoteSource,
    source::{Connection, ThreadSource},
};
use skyelab_store::Store;

#[test]
fn a_request_error_clears_once_the_reload_succeeds() {
    let mut env = Env::start();
    let port = env.port();
    drop(env.daemon.take());
    let mut source = RemoteSource::connect(NODE_NAME, options(port)).expect("client");
    let inbox = source.inbox().expect("inbox");
    source.load_threads(env.project_id); // offline: NotConnected
    let deadline = Instant::now() + Duration::from_secs(10);
    while source.error().is_none() {
        let left = deadline.saturating_duration_since(Instant::now());
        source.apply(inbox.recv_timeout(left).expect("the error"));
    }
    let backend = Arc::new(StoreBackend::new(
        Store::open(&env.db).expect("store"),
        RecordingAgents(env.agents.clone()),
    ));
    let config = Config {
        port,
        gate_timeout: Duration::from_millis(500),
        ..Config::default()
    };
    let _daemon = Daemon::start(config, env.tailnet.clone(), backend).expect("restart daemon");
    while source.connection() != Connection::Connected || source.threads(env.project_id).is_empty()
    {
        let left = deadline.saturating_duration_since(Instant::now());
        source.apply(inbox.recv_timeout(left).expect("threads after reconnect"));
    }
    assert_eq!(
        source.error(),
        None,
        "stale error after a successful reload"
    );
}
