#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use skyelab_tailnet::{Error, Tailnet, TailscaleCli};

/// A `tailscale` that hangs like a wedged tailscaled. `exec` so killing the child kills the sleep too.
fn hung_cli(tag: &str) -> (TailscaleCli, std::path::PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("skyelab-tailnet-hung-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("test setup");
    let bin = dir.join("tailscale");
    std::fs::write(&bin, "#!/bin/sh\nexec sleep 30\n").expect("test setup");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("test setup");
    let cli = TailscaleCli::with_binaries(vec![bin]).with_timeout(Duration::from_millis(300));
    (cli, dir)
}

#[test]
fn hung_cli_times_out_on_status() {
    let (cli, dir) = hung_cli("status");
    let start = Instant::now();
    let err = cli.self_node().unwrap_err();
    std::fs::remove_dir_all(&dir).expect("test setup");
    assert!(matches!(err, Error::Timeout { .. }), "{err:?}");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn hung_cli_times_out_on_whois() {
    let (cli, dir) = hung_cli("whois");
    let start = Instant::now();
    let err = cli
        .whois("100.64.0.2:4000".parse().expect("test setup"))
        .unwrap_err();
    std::fs::remove_dir_all(&dir).expect("test setup");
    assert!(matches!(err, Error::Timeout { .. }), "{err:?}");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn default_cli_has_a_timeout() {
    assert!(TailscaleCli::default().timeout() <= Duration::from_secs(10));
}
