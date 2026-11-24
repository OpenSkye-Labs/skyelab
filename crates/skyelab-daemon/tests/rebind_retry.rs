//! A port still held for a moment after the daemon stops (a just-spawned child closing its inherited listener on
//! Linux) is bound once it frees up; a port another program keeps fails, and doesn't block for long.

mod common;

use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use skyelab_daemon::Daemon;

fn held_port() -> (TcpListener, skyelab_daemon::Config) {
    let held = TcpListener::bind("127.0.0.1:0").expect("bind a free port");
    let mut config = test_config();
    config.port = held.local_addr().expect("held address").port();
    (held, config)
}

fn start_on(config: skyelab_daemon::Config) -> anyhow::Result<Daemon> {
    Daemon::start(
        config,
        Arc::new(FakeTailnet::default()),
        Arc::new(Counting::default()),
    )
}

#[test]
fn a_port_freed_a_moment_later_is_bound() {
    let (held, config) = held_port();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        drop(held);
    });
    let daemon = start_on(config);
    release.join().expect("release thread");
    assert!(daemon.is_ok(), "{:#}", daemon.err().expect("checked"));
}

#[test]
fn a_port_kept_by_another_listener_still_fails() {
    let (_held, config) = held_port();
    let started = Instant::now();
    let e = start_on(config).err().expect("the port is taken");
    assert!(
        e.chain().any(|c| c
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::AddrInUse)),
        "{e:#}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}
