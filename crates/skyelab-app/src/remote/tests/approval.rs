//! The host's side of device approval: a waiting device is listed with its code, Allow saves it and lets it in,
//! Revoke forgets it and cuts it off, Deny tells it no. A real daemon and a real `skyelab-client`.

use super::*;
use skyelab_client::{DisconnectKind, Update};
use skyelab_protocol::{device_code, device_key_hash};

const KEY: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const OTHER_KEY: &str = "2222222222222222222222222222222222222222222222222222222222222222";

struct Sharing {
    _dir: tempfile::TempDir,
    settings: PathBuf,
    remote: Entity<Remote>,
}

fn sharing(cx: &mut TestAppContext) -> (Sharing, &mut VisualTestContext) {
    cx.executor().allow_parking();
    let cx = cx.add_empty_window();
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("store")));
    let (h, _calls) = host(dir.path(), Arc::default(), store, DaemonSlot::default());
    let settings = h.settings_path.clone().expect("settings path");
    let remote = cx.new(|cx| Remote::new(h, cx));
    remote.update(cx, |r, cx| r.toggle(cx));
    wait_until(cx, "sharing", |cx| sharing_port(&remote, cx).is_some());
    (
        Sharing {
            _dir: dir,
            settings,
            remote,
        },
        cx,
    )
}

fn client(remote: &Entity<Remote>, key: &str, cx: &mut VisualTestContext) -> skyelab_client::Client {
    let port = sharing_port(remote, cx).expect("sharing");
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca().root()).expect("test CA root");
    let mut options = skyelab_client::Options::default()
        .allow_loopback()
        .trust_roots(roots)
        .resolve_to(IpAddr::V4(Ipv4Addr::LOCALHOST));
    options.port = port;
    options.backoff_min = std::time::Duration::from_millis(50);
    options.backoff_max = std::time::Duration::from_millis(200);
    options.device_key = Some(key.into());
    skyelab_client::Client::connect(NODE_NAME, options).expect("client")
}

/// Pumps until the client reports `want`, skipping other updates.
fn wait_for(
    client: &skyelab_client::Client,
    what: &str,
    want: impl Fn(&Update) -> bool,
    cx: &mut VisualTestContext,
) {
    wait_until(cx, what, |_| {
        std::iter::from_fn(|| client.updates().try_recv().ok()).any(|u| want(&u))
    });
}

fn requests(remote: &Entity<Remote>, cx: &mut VisualTestContext) -> Vec<skyelab_daemon::AccessRequest> {
    remote.update(cx, |r, cx| r.refresh_peers(cx));
    remote.read_with(cx, |r, _| r.requests.clone())
}

fn saved(path: &Path) -> Vec<(String, String)> {
    AppSettings::load(path)
        .expect("settings")
        .approved_devices
        .into_iter()
        .map(|d| (d.node_id, d.key_hash))
        .collect()
}

#[gpui::test]
fn a_waiting_device_is_listed_allowed_and_revoked(cx: &mut TestAppContext) {
    let (s, cx) = sharing(cx);
    let phone = client(&s.remote, KEY, cx);
    wait_for(
        &phone,
        "not approved",
        |u| matches!(u, Update::Disconnected(d) if d.kind == DisconnectKind::NotApproved),
        cx,
    );
    wait_until(cx, "the request", |cx| requests(&s.remote, cx).len() == 1);
    let request = requests(&s.remote, cx).remove(0);
    assert_eq!(request.device, "phone.example.ts.net");
    assert_eq!(request.user, "me@example.com");
    assert_eq!(request.code, device_code(&device_key_hash(KEY)));
    assert_eq!(
        Remote::request_label(&request),
        "phone.example.ts.net (me@example.com) wants to connect"
    );
    assert!(saved(&s.settings).is_empty());

    s.remote
        .update(cx, |r, cx| r.allow(&request.node_id, &request.key_hash, cx));
    assert_eq!(
        saved(&s.settings),
        vec![("nPhone".to_owned(), device_key_hash(KEY))]
    );
    wait_for(&phone, "connected", |u| matches!(u, Update::Connected(_)), cx);
    assert!(requests(&s.remote, cx).is_empty());
    let approved = s.remote.read_with(cx, |r, _| r.approved.clone());
    assert_eq!(approved.len(), 1);
    assert_eq!(approved[0].name, "phone.example.ts.net");

    s.remote
        .update(cx, |r, cx| r.revoke("nPhone", &device_key_hash(KEY), cx));
    assert!(saved(&s.settings).is_empty());
    wait_for(
        &phone,
        "cut off",
        |u| matches!(u, Update::Disconnected(d) if d.kind == DisconnectKind::NotApproved),
        cx,
    );
}

#[gpui::test]
fn deny_tells_the_device_and_saves_nothing(cx: &mut TestAppContext) {
    let (s, cx) = sharing(cx);
    let other = client(&s.remote, OTHER_KEY, cx);
    wait_until(cx, "the request", |cx| requests(&s.remote, cx).len() == 1);
    let request = requests(&s.remote, cx).remove(0);
    s.remote
        .update(cx, |r, cx| r.deny(&request.node_id, &request.key_hash, cx));
    assert!(requests(&s.remote, cx).is_empty());
    wait_for(
        &other,
        "denied",
        |u| matches!(u, Update::Disconnected(d) if d.kind == DisconnectKind::Denied),
        cx,
    );
    assert!(saved(&s.settings).is_empty());
}

#[gpui::test]
fn saved_approvals_apply_when_sharing_starts_again(cx: &mut TestAppContext) {
    let (s, cx) = sharing(cx);
    let phone = client(&s.remote, KEY, cx);
    wait_until(cx, "the request", |cx| requests(&s.remote, cx).len() == 1);
    s.remote
        .update(cx, |r, cx| r.allow("nPhone", &device_key_hash(KEY), cx));
    wait_for(&phone, "connected", |u| matches!(u, Update::Connected(_)), cx);
    drop(phone);
    s.remote.update(cx, |r, cx| {
        r.toggle(cx);
        r.toggle(cx);
    });
    wait_until(cx, "sharing again", |cx| sharing_port(&s.remote, cx).is_some());
    let phone = client(&s.remote, KEY, cx);
    wait_for(&phone, "connected", |u| matches!(u, Update::Connected(_)), cx);
}
