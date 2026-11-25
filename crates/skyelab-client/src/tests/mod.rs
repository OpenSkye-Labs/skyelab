//! Tests against a real in-process `skyelab-daemon` (fake tailnet, loopback sockets) and hostile fake servers.
//! They live in the crate because the loopback exception only exists under `cfg(test)`.

mod approval;
mod approve_gate;
mod backoff;
mod big_events;
mod disconnect_kind;
mod events;
mod fill_retry;
mod forward;
mod guard;
mod hello;
mod hostile;
mod keepalive;
mod keychain;
mod reconnect;
mod rpc;
mod shelve;
mod tls;
mod watch_handles;

use std::sync::Arc;
use std::time::Duration;

pub use skyelab_daemon::testing::{Env, FakeTailnet, NODE_NAME, Peer};
use skyelab_daemon::testing::{ca, config};
use skyelab_daemon::{Backend, Daemon};

use crate::{Client, Options, Update};

/// The name every test dials. `options` points it at loopback; TLS still checks the certificate names it.
pub const HOST: &str = NODE_NAME;

/// Appends `n` events to the env's thread and returns the last one, as the daemon would push it.
pub fn append(env: &Env, n: usize) -> skyelab_protocol::ThreadEvent {
    let mut last = None;
    for _ in 0..n {
        last = Some(env.append(env.thread_id, "assistant", "{}"));
    }
    last.expect("n > 0")
}

/// The device key [`options`] sends, which the harness's daemon config approves.
pub const TEST_KEY: &str = crate::testing::DEVICE_KEY;

pub fn daemon_config(port: u16) -> skyelab_daemon::Config {
    config(port)
}

pub fn start_daemon(tailnet: Arc<FakeTailnet>, backend: Arc<dyn Backend>, port: u16) -> Daemon {
    Daemon::start(config(port), tailnet, backend).expect("start daemon")
}

/// Options for a loopback test server: HOST dials 127.0.0.1, the test CA is trusted, quick backoff.
pub fn options(port: u16) -> Options {
    crate::testing::options(port, ca().root())
}

pub fn port(daemon: &Daemon) -> u16 {
    daemon.local_addrs()[0].port()
}

/// The next update, failing the test after 60 s (a loaded machine can take many seconds to connect).
pub fn next(client: &Client) -> Update {
    client
        .updates()
        .recv_timeout(Duration::from_secs(60))
        .expect("no update within 60 s")
}

pub fn connected(client: &Client) {
    match next(client) {
        Update::Connected(_) => {}
        other => panic!("expected Connected, got {other:?}"),
    }
}

/// Seqs of the next `n` updates, which must all be events.
pub fn seqs(client: &Client, n: usize) -> Vec<i64> {
    (0..n)
        .map(|_| match next(client) {
            Update::Event(ev) => ev.seq,
            other => panic!("expected an event, got {other:?}"),
        })
        .collect()
}

/// Nothing more arrives for `d`.
pub fn quiet(client: &Client, d: Duration) {
    if let Ok(update) = client.updates().recv_timeout(d) {
        panic!("expected no update, got {update:?}");
    }
}
