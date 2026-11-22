//! Sharing needs this node's TLS certificate: without one it refuses and says how to fix it.

use super::*;

#[gpui::test]
fn cert_unavailable_shows_fix(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let cx = cx.add_empty_window();
    let dir = tempfile::tempdir().expect("tempdir");
    let tailnet = Arc::new(FakeTailnet::default());
    tailnet.certs_off.store(true, Ordering::SeqCst);
    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("store")));
    let slot = DaemonSlot::default();
    let (h, _calls) = host(dir.path(), tailnet, store, slot.clone());
    let remote = cx.new(|cx| Remote::new(h, cx));
    remote.update(cx, |r, cx| r.toggle(cx));
    wait_until(cx, "the start to fail", |cx| {
        remote.read_with(cx, |r, _| matches!(r.status, Status::Failed(_)))
    });
    let label = remote.read_with(cx, |r, _| r.status.label());
    for want in [
        "Not sharing",
        "desktop.example.ts.net",
        "HTTPS Certificates",
        skyelab_daemon::ADMIN_DNS_URL,
    ] {
        assert!(label.contains(want), "{want:?} missing from {label}");
    }
    assert!(slot.lock().expect("slot").is_none(), "no daemon runs");
}

#[gpui::test]
fn sharing_shows_name_and_cert_expiry(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let cx = cx.add_empty_window();
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("store")));
    let (h, _calls) = host(dir.path(), Arc::default(), store, DaemonSlot::default());
    let remote = cx.new(|cx| Remote::new(h, cx));
    remote.update(cx, |r, cx| r.toggle(cx));
    wait_until(cx, "sharing", |cx| sharing_port(&remote, cx).is_some());
    remote.read_with(cx, |r, _| match &r.status {
        Status::Sharing {
            name, cert_until, ..
        } => {
            assert_eq!(name, NODE_NAME);
            assert_eq!(cert_until, "2099-01-01 00:00 UTC");
        }
        other => panic!("{other:?}"),
    });
}
