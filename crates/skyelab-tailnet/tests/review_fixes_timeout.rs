#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use skyelab_tailnet::{Tailnet, TailscaleCli, is_own_device, parse_status, parse_whois};

/// whois fails fast, then the status follow-up hangs: the whole whois() must still end within one timeout.
#[test]
fn whois_and_its_status_follow_up_share_one_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("tailscale");
    std::fs::write(
        &bin,
        "#!/bin/sh\nif [ \"$1\" = whois ]; then sleep 0.6; echo nope >&2; exit 1; fi\nexec sleep 30\n",
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let cli = TailscaleCli::with_binaries(vec![bin]).with_timeout(Duration::from_secs(1));
    let started = Instant::now();
    assert!(cli.whois("100.64.0.2:4000".parse().unwrap()).is_err());
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "took {:?}",
        started.elapsed()
    );
}

#[test]
fn own_device_rejects_whois_without_node_id() {
    let fixture = |n: &str| {
        std::fs::read(format!("{}/tests/fixtures/{n}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    };
    let me = parse_status(&fixture("status_running.json"))
        .unwrap()
        .self_node;
    let mut w = parse_whois(&fixture("whois_own.json")).unwrap();
    assert!(is_own_device(&me, &w));
    w.node.stable_id.clear();
    assert!(!is_own_device(&me, &w));
}
