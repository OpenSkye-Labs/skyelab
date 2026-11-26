#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use skyelab_tailnet::{Tailnet, TailscaleCli};

/// The CLI starts from the shared allowlist, not SkyeLab's whole env: DO_NOT_TRACK=1, no stray vars, PATH kept.
#[test]
fn cli_gets_the_allowlisted_env() {
    let dir = tempfile::tempdir().expect("test setup");
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/status_running.json"
    );
    let env_out = dir.path().join("env.txt");
    let bin = dir.path().join("tailscale");
    // `env` and `cat` are bare names: they only run if PATH reached the child.
    let script = format!(
        "#!/bin/sh\nenv > '{}'\ncat '{fixture}'\n",
        env_out.display()
    );
    std::fs::write(&bin, script).expect("test setup");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("test setup");

    TailscaleCli::with_binaries(vec![bin])
        .self_node()
        .expect("the fake CLI ran");

    let env = std::fs::read_to_string(&env_out).expect("the fake CLI wrote its env");
    assert!(env.lines().any(|l| l == "DO_NOT_TRACK=1"), "{env}");
    // Cargo sets this for the test process; it is not on the allowlist.
    assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
    assert!(!env.contains("CARGO_MANIFEST_DIR="), "{env}");
}
