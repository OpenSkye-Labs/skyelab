use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use skyelab_tailnet::{
    Error, Node, Tailnet, TailscaleCli, WhoIs, is_own_device, parse_status, parse_whois,
};

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).expect("test setup")
}

fn self_node() -> Node {
    parse_status(&fixture("status_running.json"))
        .expect("test setup")
        .self_node
}

fn whois(name: &str) -> WhoIs {
    parse_whois(&fixture(name)).expect("test setup")
}

#[test]
fn parses_running_status() {
    let status = parse_status(&fixture("status_running.json")).expect("test setup");
    let me = &status.self_node;
    assert_eq!(me.stable_id, "nSelf111CNTRL");
    assert_eq!(me.hostname, "laptop");
    assert_eq!(me.dns_name, "laptop.example.ts.net.");
    assert_eq!(
        me.tailscale_ips,
        vec![
            "100.64.0.1"
                .parse::<std::net::IpAddr>()
                .expect("test setup"),
            "fd7a:115c:a1e0::1".parse().expect("test setup")
        ]
    );
    assert_eq!(me.os, "macOS");
    assert!(me.online);
    assert_eq!(me.user_id, Some(1001));
    assert!(me.tags.is_empty());

    assert_eq!(status.peers.len(), 2);
    let phone = status
        .peers
        .iter()
        .find(|n| n.stable_id == "nPhone22CNTRL")
        .expect("test setup");
    assert!(phone.tags.is_empty(), "null Tags parse as no tags");
    assert!(!phone.online);
    let server = status
        .peers
        .iter()
        .find(|n| n.stable_id == "nServer3CNTRL")
        .expect("test setup");
    assert_eq!(server.tags, vec!["tag:server"]);
}

#[test]
fn parses_whois() {
    let w = whois("whois_own.json");
    assert_eq!(w.node.stable_id, "nPhone22CNTRL");
    assert_eq!(w.node.user, Some(1001));
    assert_eq!(w.user_profile.id, Some(1001));
    assert_eq!(w.user_profile.login_name, "me@example.com");
}

#[test]
fn stopped_status_is_not_running() {
    let err = parse_status(&fixture("status_stopped.json")).unwrap_err();
    assert!(
        matches!(err, Error::NotRunning(ref s) if s == "Stopped"),
        "{err:?}"
    );
}

#[test]
fn needs_login_status_is_not_running() {
    let err = parse_status(&fixture("status_needs_login.json")).unwrap_err();
    assert!(
        matches!(err, Error::NotRunning(ref s) if s == "NeedsLogin"),
        "{err:?}"
    );
}

#[test]
fn own_device_same_user_untagged() {
    assert!(is_own_device(&self_node(), &whois("whois_own.json")));
}

#[test]
fn own_device_rejects_tagged_node_even_for_same_user() {
    assert!(!is_own_device(&self_node(), &whois("whois_tagged.json")));
}

#[test]
fn own_device_rejects_other_user() {
    assert!(!is_own_device(
        &self_node(),
        &whois("whois_other_user.json")
    ));
}

#[test]
fn own_device_rejects_missing_user_fields() {
    assert!(!is_own_device(
        &self_node(),
        &whois("whois_missing_user.json")
    ));

    let mut me = self_node();
    me.user_id = None;
    assert!(!is_own_device(&me, &whois("whois_own.json")));

    let mut w = whois("whois_own.json");
    w.node.user = None;
    assert!(
        !is_own_device(&self_node(), &w),
        "node owner must be present"
    );
}

#[test]
fn own_device_rejects_zero_user_ids() {
    // Go's zero value: a logged-out self (UserID 0) must not match an empty profile.
    let mut me = self_node();
    me.user_id = Some(0);
    let mut w = whois("whois_own.json");
    w.user_profile.id = Some(0);
    w.node.user = Some(0);
    assert!(!is_own_device(&me, &w));
}

#[test]
fn own_device_rejects_profile_node_owner_mismatch() {
    let mut w = whois("whois_own.json");
    w.node.user = Some(2002);
    assert!(!is_own_device(&self_node(), &w));
}

#[test]
fn missing_binary_is_not_installed() {
    let cli = TailscaleCli::with_binaries(vec![PathBuf::from("/nonexistent/tailscale")]);
    assert!(matches!(cli.self_node(), Err(Error::NotInstalled)));
}

/// A fake `tailscale` that prints a fixture, so the trait is driven end to end without a tailnet.
#[cfg(unix)]
fn fake_cli(status_fixture: &str, whois_fixture: &str) -> (TailscaleCli, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let dir = std::env::temp_dir().join(format!(
        "skyelab-tailnet-{}-{status_fixture}-{whois_fixture}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("test setup");
    let bin = dir.join("tailscale");
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\n  status) cat '{s}' ;;\n  whois) [ \"$2\" = --json ] && [ \"$3\" = 100.64.0.2:4000 ] && cat '{w}' || {{ echo 'peer not found' >&2; exit 1; }} ;;\nesac\n",
        s = fixtures.join(status_fixture).display(),
        w = fixtures.join(whois_fixture).display(),
    );
    std::fs::write(&bin, script).expect("test setup");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("test setup");
    (
        TailscaleCli::with_binaries(vec![PathBuf::from("/nonexistent/tailscale"), bin]),
        dir,
    )
}

#[cfg(unix)]
#[test]
fn cli_backend_through_trait() {
    let (cli, dir) = fake_cli("status_running.json", "whois_own.json");
    let me = cli.self_node().expect("test setup");
    assert_eq!(me.hostname, "laptop");
    assert_eq!(cli.peers().expect("test setup").len(), 2);
    let addr: SocketAddr = "100.64.0.2:4000".parse().expect("test setup");
    let w = cli.whois(addr).expect("test setup");
    assert!(is_own_device(&me, &w));

    let unknown: SocketAddr = "100.64.0.9:4000".parse().expect("test setup");
    assert!(matches!(cli.whois(unknown), Err(Error::Command { .. })));
    std::fs::remove_dir_all(dir).expect("test setup");
}

#[cfg(unix)]
#[test]
fn cli_backend_stopped() {
    let (cli, dir) = fake_cli("status_stopped.json", "whois_own.json");
    assert!(matches!(cli.peers(), Err(Error::NotRunning(_))));
    std::fs::remove_dir_all(dir).expect("test setup");
}
