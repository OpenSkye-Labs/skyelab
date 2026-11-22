#![cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use skyelab_tailnet::{Tailnet, TailscaleCli};

#[test]
fn cert_timeout_reports_the_cert_budget() {
    let dir = tempfile::tempdir().expect("temp dir");
    let name = "laptop.example.ts.net";
    let status = format!(
        r#"{{"BackendState":"Running","Self":{{"ID":"nSelf","DNSName":"{name}.","UserID":1,"TailscaleIPs":["100.64.0.1"]}},"CertDomains":["{name}"]}}"#
    );
    let bin = dir.path().join("tailscale");
    std::fs::write(
        &bin,
        format!("#!/bin/sh\ncase \"$1\" in\n  status) echo '{status}' ;;\n  cert) exec sleep 30 ;;\nesac\n"),
    )
    .expect("write fake cli");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let cli = TailscaleCli::with_binaries(vec![bin])
        .with_cert_dir(dir.path().join("tls"))
        .with_cert_timeout(Duration::from_millis(500));
    let msg = cli.cert(name).unwrap_err().to_string();
    assert!(
        !msg.contains("within 5s"),
        "BUG wrong budget in message: {msg}"
    );
}
