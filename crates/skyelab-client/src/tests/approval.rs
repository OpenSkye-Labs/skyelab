//! A daemon that hasn't approved this device says so in words the client types: `NotApproved`, or `Denied` once
//! the user turned it down, and an Allow takes effect within a few seconds.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::DisconnectKind;
use skyelab_protocol::{device_code, device_key_hash};

fn kind(client: &Client) -> DisconnectKind {
    match next(client) {
        Update::Disconnected(d) => d.kind,
        other => panic!("expected Disconnected, got {other:?}"),
    }
}

#[test]
fn an_unapproved_device_is_told_not_approved_with_its_code() {
    let env = Env::new();
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), env.backend(), 0);
    daemon.set_approved(vec![]);
    let client = Client::connect(HOST, options(port(&daemon))).expect("client");
    let d = match next(&client) {
        Update::Disconnected(d) => d,
        other => panic!("expected Disconnected, got {other:?}"),
    };
    assert_eq!(d.kind, DisconnectKind::NotApproved, "{d:?}");
    let code = device_code(&device_key_hash(TEST_KEY));
    assert!(d.message.contains(&code), "{d:?}");
    assert_eq!(client.device_code().as_deref(), Some(code.as_str()));
}

#[test]
fn a_denied_device_is_told_denied() {
    let env = Env::new();
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), env.backend(), 0);
    daemon.set_approved(vec![]);
    daemon.deny("nPhone", &device_key_hash(TEST_KEY));
    let client = Client::connect(HOST, options(port(&daemon))).expect("client");
    assert_eq!(kind(&client), DisconnectKind::Denied);
}

#[test]
fn allowing_a_waiting_device_connects_it_within_seconds() {
    let env = Env::new();
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), env.backend(), 0);
    daemon.set_approved(vec![]);
    let mut opts = options(port(&daemon));
    // A long backoff, as in a real build: being "not approved" must not wait that long between tries.
    opts.backoff_min = Duration::from_millis(500);
    opts.backoff_max = Duration::from_secs(60);
    let client = Client::connect(HOST, opts).expect("client");
    // Several refusals push the normal backoff past 4 s.
    for _ in 0..4 {
        assert_eq!(kind(&client), DisconnectKind::NotApproved);
    }
    daemon.set_approved(vec![skyelab_daemon::Approval {
        node_id: "nPhone".into(),
        key_hash: device_key_hash(TEST_KEY),
    }]);
    let allowed = Instant::now();
    loop {
        match client.updates().recv_timeout(Duration::from_secs(5)) {
            Ok(Update::Connected(_)) => break,
            Ok(Update::Disconnected(_)) => continue,
            other => panic!("expected to connect, got {other:?}"),
        }
    }
    assert!(allowed.elapsed() < Duration::from_secs(4), "{:?}", allowed.elapsed());
}

#[test]
fn a_client_without_a_key_is_not_approved() {
    let env = Env::new();
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), env.backend(), 0);
    let mut opts = options(port(&daemon));
    opts.device_key = None;
    let client = Client::connect(HOST, opts).expect("client");
    assert_eq!(kind(&client), DisconnectKind::NotApproved);
    assert_eq!(client.device_code(), None);
}

#[test]
fn the_device_key_file_is_made_once_private_and_reused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("nested").join("device-key");
    let key = crate::load_or_create_device_key(&path).expect("create");
    assert!(skyelab_protocol::is_device_key(&key), "{key}");
    assert_eq!(crate::load_or_create_device_key(&path).expect("load"), key);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
        assert_eq!(mode & 0o077, 0, "only the owner may read it: {mode:o}");
    }
    // A file that isn't a key is an error, never silently replaced (that would un-approve the device).
    std::fs::write(&path, "garbage").expect("write");
    assert!(crate::load_or_create_device_key(&path).is_err());
}
