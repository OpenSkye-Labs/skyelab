//! Which tailnet peers the environment picker offers.

use super::*;

fn node(id: &str, dns: &str, user: Option<UserId>, online: bool, tags: &[&str]) -> Node {
    Node {
        stable_id: id.into(),
        hostname: "host".into(),
        dns_name: dns.into(),
        tailscale_ips: vec![],
        os: "linux".into(),
        online,
        user_id: user,
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
    }
}

#[test]
fn own_online_peers_keeps_only_your_untagged_online_devices() {
    let me = node("nSelf", "desktop.ts.net.", Some(7), true, &[]);
    let peers = vec![
        node("nLaptop", "laptop.ts.net.", Some(7), true, &[]),
        node("nOff", "off.ts.net.", Some(7), false, &[]),
        node("nTagged", "server.ts.net.", Some(7), true, &["tag:server"]),
        node("nOther", "friend.ts.net.", Some(8), true, &[]),
        node("nNoUser", "nobody.ts.net.", None, true, &[]),
        node("nSelf", "desktop.ts.net.", Some(7), true, &[]),
        node("", "anon.ts.net.", Some(7), true, &[]),
    ];
    let kept: Vec<String> = own_online_peers(&me, peers)
        .iter()
        .map(|n| n.magic_dns_name().to_owned())
        .collect();
    assert_eq!(kept, ["laptop.ts.net"]);
}

#[test]
fn own_online_peers_is_empty_for_a_tagged_or_userless_self() {
    let peer = node("nLaptop", "laptop.ts.net.", Some(7), true, &[]);
    let tagged = node("nSelf", "d.ts.net.", Some(7), true, &["tag:ci"]);
    assert!(own_online_peers(&tagged, vec![peer.clone()]).is_empty());
    let nobody = node("nSelf", "d.ts.net.", Some(0), true, &[]);
    assert!(own_online_peers(&nobody, vec![peer]).is_empty());
}

#[test]
fn magic_dns_name_falls_back_to_the_hostname() {
    let n = node("n", "", Some(7), true, &[]);
    assert_eq!(n.magic_dns_name(), "host");
}
