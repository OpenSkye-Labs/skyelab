use skyelab_tailnet::{Node, WhoIs, is_own_device, parse_status, parse_whois};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("test setup")
}

fn me() -> Node {
    parse_status(&fixture("status_running.json"))
        .expect("test setup")
        .self_node
}

fn own_phone() -> WhoIs {
    parse_whois(&fixture("whois_own.json")).expect("test setup")
}

#[test]
fn own_device_rejects_this_node_itself() {
    // Another OS user on this machine connecting through our own tailnet IP is not "one of my devices".
    let me = me();
    let mut w = own_phone();
    w.node.stable_id = me.stable_id.clone();
    assert!(!is_own_device(&me, &w));
}

#[test]
fn own_device_rejects_when_self_is_tagged() {
    // A tagged machine has no personal owner, so nothing is "its user's" device.
    let mut me = me();
    me.tags = vec!["tag:server".into()];
    assert!(!is_own_device(&me, &own_phone()));
}

#[cfg(unix)]
#[test]
fn whois_on_unreachable_daemon_is_not_running() {
    use skyelab_tailnet::{Error, Tailnet, TailscaleCli};
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!("skyelab-tailnet-down-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("test setup");
    let bin = dir.join("tailscale");
    std::fs::write(
        &bin,
        "#!/bin/sh\necho 'failed to connect to local tailscaled' >&2\nexit 1\n",
    )
    .expect("test setup");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("test setup");
    let cli = TailscaleCli::with_binaries(vec![bin]);

    let err = cli
        .whois("100.64.0.2:4000".parse().expect("test setup"))
        .unwrap_err();
    std::fs::remove_dir_all(&dir).expect("test setup");
    assert!(matches!(err, Error::NotRunning(_)), "{err:?}");
}
