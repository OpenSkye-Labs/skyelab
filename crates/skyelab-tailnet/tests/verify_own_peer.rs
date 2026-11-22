//! The client-side identity check: this machine's own Tailscale says who a dialled address is.

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use skyelab_tailnet::{Error, Node, Tailnet, UserProfile, WhoIs, WhoIsNode, verify_own_peer};

const ME: u64 = 7;

struct Fake {
    whois: fn() -> Result<WhoIs, Error>,
}

impl Tailnet for Fake {
    fn self_node(&self) -> Result<Node, Error> {
        Ok(Node {
            stable_id: "nClient".into(),
            hostname: "laptop".into(),
            dns_name: "laptop.example.ts.net.".into(),
            tailscale_ips: vec![],
            os: "macOS".into(),
            online: true,
            user_id: Some(ME),
            tags: vec![],
        })
    }
    fn peers(&self) -> Result<Vec<Node>, Error> {
        Ok(vec![])
    }
    fn whois(&self, _: SocketAddr) -> Result<WhoIs, Error> {
        (self.whois)()
    }
}

fn whois(id: &str, user: u64, tags: &[&str]) -> WhoIs {
    WhoIs {
        node: WhoIsNode {
            stable_id: id.into(),
            name: "desktop.example.ts.net.".into(),
            user: Some(user),
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        },
        user_profile: UserProfile {
            id: Some(user),
            login_name: "me@example.com".into(),
            display_name: "Me".into(),
        },
    }
}

fn check(f: fn() -> Result<WhoIs, Error>) -> Result<String, String> {
    verify_own_peer(
        &Fake { whois: f },
        SocketAddr::from((Ipv4Addr::new(100, 64, 0, 2), 7287)),
    )
}

#[test]
fn your_own_untagged_device_gives_its_stable_id() {
    assert_eq!(
        check(|| Ok(whois("nDesktop", ME, &[]))),
        Ok("nDesktop".into())
    );
}

#[test]
fn another_users_device_is_refused() {
    let err = check(|| Ok(whois("nFriend", ME + 1, &[]))).expect_err("refused");
    assert!(err.contains("nFriend"), "{err}");
}

#[test]
fn a_tagged_device_is_refused_even_if_you_tagged_it() {
    assert!(check(|| Ok(whois("nServer", ME, &["tag:server"]))).is_err());
}

#[test]
fn this_machine_itself_is_refused() {
    assert!(check(|| Ok(whois("nClient", ME, &[]))).is_err());
}

#[test]
fn a_node_without_an_id_is_refused() {
    assert!(check(|| Ok(whois("", ME, &[]))).is_err());
}

#[test]
fn whois_errors_and_timeouts_fail_closed() {
    let err = check(|| {
        Err(Error::Timeout {
            command: "whois",
            after: Duration::from_secs(5),
        })
    })
    .expect_err("refused");
    assert!(err.contains("did not answer"), "{err}");
    assert!(check(|| Err(Error::NotInstalled)).is_err());
}
