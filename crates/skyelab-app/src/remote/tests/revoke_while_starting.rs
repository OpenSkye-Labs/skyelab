//! A Revoke clicked while sharing is still starting (the daemon not running yet) still keeps that device out
//! once it runs.

use super::*;
use skyelab_client::{DisconnectKind, Update};
use skyelab_protocol::device_key_hash;

const KEY: &str = "3333333333333333333333333333333333333333333333333333333333333333";

#[gpui::test]
fn a_revoke_while_sharing_starts_still_keeps_the_device_out(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let cx = cx.add_empty_window();
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("store")));
    let (h, _calls) = host(dir.path(), Arc::default(), store, DaemonSlot::default());
    let settings = h.settings_path.clone().expect("settings path");
    AppSettings {
        approved_devices: vec![skyelab_core::ApprovedDevice {
            node_id: "nPhone".into(),
            key_hash: device_key_hash(KEY),
            name: "phone.example.ts.net".into(),
            user: "me@example.com".into(),
        }],
        ..AppSettings::default()
    }
    .save(&settings)
    .expect("save");
    let remote = cx.new(|cx| Remote::new(h, cx));
    remote.update(cx, |r, cx| {
        r.toggle(cx); // Starting: the daemon isn't in the slot yet
        r.revoke("nPhone", &device_key_hash(KEY), cx);
    });
    wait_until(cx, "sharing", |cx| sharing_port(&remote, cx).is_some());
    let port = sharing_port(&remote, cx).expect("sharing");
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca().root()).expect("test CA root");
    let mut options = skyelab_client::Options::default()
        .allow_loopback()
        .trust_roots(roots)
        .resolve_to(IpAddr::V4(Ipv4Addr::LOCALHOST));
    options.port = port;
    options.backoff_min = std::time::Duration::from_millis(50);
    options.backoff_max = std::time::Duration::from_millis(200);
    options.device_key = Some(KEY.into());
    let phone = skyelab_client::Client::connect(NODE_NAME, options).expect("client");
    let mut outcome = None;
    wait_until(cx, "the hello outcome", |_| {
        while let Ok(u) = phone.updates().try_recv() {
            match u {
                Update::Connected(_) => outcome = Some("connected"),
                Update::Disconnected(d) if d.kind == DisconnectKind::NotApproved => {
                    outcome = Some("not approved")
                }
                _ => {}
            }
        }
        outcome.is_some()
    });
    assert_eq!(outcome, Some("not approved"), "the revoked device got in");
}
