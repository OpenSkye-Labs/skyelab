//! The listener sits on the tailnet IPs only, and nothing starts without a tailnet.

mod common;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;

use common::*;
use skyelab_daemon::Daemon;

fn tailnet_with(ips: Vec<IpAddr>, down: bool) -> Arc<FakeTailnet> {
    Arc::new(FakeTailnet {
        ips,
        down: down.into(),
        ..FakeTailnet::default()
    })
}

#[test]
fn refuses_to_start_when_tailnet_down() {
    let t = tailnet_with(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)], true);
    assert!(Daemon::start(test_config(), t, Arc::new(Counting::default())).is_err());
}

#[test]
fn binds_only_tailnet_ips() {
    let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let daemon = Daemon::start(
        test_config(),
        tailnet_with(vec![ip], false),
        Arc::new(Counting::default()),
    )
    .expect("test setup");
    assert_eq!(daemon.local_addrs().len(), 1);
    assert!(
        daemon.local_addrs().iter().all(|a| a.ip() == ip),
        "{:?}",
        daemon.local_addrs()
    );
}

#[test]
fn refuses_wildcard_addresses() {
    for ip in [
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    ] {
        let t = tailnet_with(vec![IpAddr::V4(Ipv4Addr::LOCALHOST), ip], false);
        assert!(
            Daemon::start(test_config(), t, Arc::new(Counting::default())).is_err(),
            "{ip}"
        );
    }
}

#[test]
fn refuses_to_start_without_tailnet_ips() {
    let t = tailnet_with(vec![], false);
    assert!(Daemon::start(test_config(), t, Arc::new(Counting::default())).is_err());
}

#[test]
fn shutdown_closes_the_port() {
    let t = tailnet_with(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)], false);
    let daemon =
        Daemon::start(test_config(), t, Arc::new(Counting::default())).expect("test setup");
    let addr = daemon.local_addrs()[0];
    drop(daemon);
    assert!(std::net::TcpStream::connect(addr).is_err());
}
