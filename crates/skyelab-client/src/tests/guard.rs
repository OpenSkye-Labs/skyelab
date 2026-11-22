//! Only tailnet addresses are dialed, resolved names included.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::guard::allowed;
use crate::is_tailnet_ip;

fn ip(s: &str) -> IpAddr {
    s.parse().expect("test ip")
}

#[test]
fn refuses_addresses_outside_the_tailnet() {
    for s in [
        "8.8.8.8",
        "192.168.1.10",
        "10.0.0.1",
        "100.63.255.255",
        "100.128.0.1",
        "127.0.0.1",
        "::1",
        "2001:4860:4860::8888",
        "fd7a:115c:a1e1::1",
        "::ffff:127.0.0.1",
        "::ffff:8.8.8.8",
    ] {
        assert!(!is_tailnet_ip(ip(s)), "{s} must be refused");
    }
}

#[test]
fn allows_tailnet_addresses() {
    for s in [
        "100.64.0.1",
        "100.101.102.103",
        "100.127.255.254",
        "fd7a:115c:a1e0::1",
        "fd7a:115c:a1e0:ab12:4843:cd96:6258:b240",
        "::ffff:100.101.102.103",
    ] {
        assert!(is_tailnet_ip(ip(s)), "{s} must be allowed");
    }
}

#[test]
fn allowed_keeps_only_checked_addresses() {
    let resolved: Vec<SocketAddr> = ["8.8.8.8:1", "100.100.1.1:1", "[fd7a:115c:a1e0::2]:1"]
        .iter()
        .map(|s| s.parse().expect("test addr"))
        .collect();
    let ok = allowed("box", resolved.clone(), false).expect("tailnet addrs present");
    assert_eq!(ok, resolved[1..]);
    let err = allowed("box", resolved[..1].to_vec(), false).expect_err("public only");
    assert!(err.contains("8.8.8.8"), "{err}");
}

#[test]
fn loopback_switch_adds_only_loopback() {
    let public: SocketAddr = "8.8.8.8:1".parse().expect("test addr");
    let lan: SocketAddr = "192.168.0.2:1".parse().expect("test addr");
    assert!(allowed("x", [public, lan], true).is_err());
    let lo: SocketAddr = "127.0.0.1:1".parse().expect("test addr");
    assert_eq!(allowed("x", [public, lo], true), Ok(vec![lo]));
    assert!(allowed("x", [lo], false).is_err());
}

#[test]
fn refuses_a_loopback_name_before_connecting() {
    let tailnet = Arc::new(FakeTailnet::default());
    let env = Env::new();
    let daemon = start_daemon(tailnet.clone(), env.backend(), 0);
    let opts = Options {
        allow_loopback: false,
        ..options(port(&daemon))
    };
    let client = Client::connect("localhost", opts).expect("client");
    match next(&client) {
        Update::Disconnected(reason) => assert!(reason.message.contains("refusing"), "{reason}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        tailnet.whois_calls(),
        0,
        "no TCP connection reached the daemon"
    );
    assert!(client.hello().is_none());
}
