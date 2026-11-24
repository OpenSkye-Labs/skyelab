//! One tailnet member can't lock the user's own devices out of the gate: whois slots are bounded per source
//! address, and a peer refused as someone else's is dropped without another whois for a while.

mod common;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use common::*;
use skyelab_daemon::testing::Peer;

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const V6: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);

fn two_address_tailnet() -> Arc<FakeTailnet> {
    Arc::new(FakeTailnet {
        ips: vec![V4, V6],
        ..FakeTailnet::default()
    })
}

fn wait_for_whois(tailnet: &FakeTailnet, n: usize) {
    let until = std::time::Instant::now() + Duration::from_secs(60);
    while std::time::Instant::now() < until {
        if tailnet.whois_calls() >= n {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("only {} whois calls ran, wanted {n}", tailnet.whois_calls());
}

#[test]
fn a_flooding_peer_cannot_starve_another_peers_admission() {
    let tailnet = two_address_tailnet();
    // The flooder's whois hangs well past the gate timeout, so each of its connections keeps a slot.
    tailnet
        .peer_by_ip
        .lock()
        .expect("lock")
        .insert(V4, Peer::Hang(Duration::from_secs(4)));
    let daemon = start(
        tailnet.clone(),
        Arc::new(Counting::default()),
        short_gate_config(),
    );
    let (flood_addr, own_addr) = (daemon.local_addrs()[0], daemon.local_addrs()[1]);
    assert_eq!((flood_addr.ip(), own_addr.ip()), (V4, V6));

    // More connections than the whole pool (16).
    let _flood: Vec<TcpStream> = (0..24)
        .map(|_| TcpStream::connect(flood_addr).expect("flood connect"))
        .collect();
    wait_for_whois(&tailnet, 4);
    std::thread::sleep(Duration::from_millis(300)); // let the accept loop take every flood connection
    let flood_whois = tailnet.whois_calls();
    assert!(
        flood_whois < 16,
        "one peer ran {flood_whois} whois calls at once; it must not fill the pool"
    );

    let admitted = try_connect(own_addr);
    assert!(
        admitted.is_ok(),
        "the user's own device was locked out: {:?}",
        admitted.err()
    );
}

#[test]
fn a_refused_peer_is_dropped_without_another_whois() {
    let tailnet = two_address_tailnet();
    tailnet
        .peer_by_ip
        .lock()
        .expect("lock")
        .insert(V4, Peer::OtherUser);
    let daemon = start(
        tailnet.clone(),
        Arc::new(Counting::default()),
        test_config(),
    );
    let (stranger, own) = (daemon.local_addrs()[0], daemon.local_addrs()[1]);

    assert!(try_connect(stranger).is_err(), "another user is refused");
    assert_eq!(tailnet.whois_calls(), 1);
    for _ in 0..3 {
        assert!(try_connect(stranger).is_err());
    }
    assert_eq!(
        tailnet.whois_calls(),
        1,
        "a peer refused moments ago gets no new whois"
    );

    // Another address is not affected.
    assert!(try_connect(own).is_ok(), "own device admitted");
    assert_eq!(tailnet.whois_calls(), 2);
}
