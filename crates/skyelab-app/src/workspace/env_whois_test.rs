//! The client checks the dialled machine with its OWN Tailscale (whois), so the node it pins is not the server's
//! word: another user, a tagged node, a failed whois or a hello that claims another node is never connected to.

use super::env_harness::*;
use super::env_pin_test::nodeless_server;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_core::AppSettings;
use skyelab_daemon::testing::NODE_NAME;
use skyelab_tailnet::{Error as TailnetError, Node, Tailnet, UserProfile, WhoIs, WhoIsNode};
use std::net::SocketAddr;
use std::time::Duration;

/// The client machine's own Tailscale: this node is `nClient` (user ME), and whois answers `answer`.
struct ClientTailnet {
    answer: fn() -> Result<WhoIs, TailnetError>,
}

impl Tailnet for ClientTailnet {
    fn self_node(&self) -> Result<Node, TailnetError> {
        Ok(node("nClient", "laptop.example.ts.net", ME, true, &[]))
    }
    fn peers(&self) -> Result<Vec<Node>, TailnetError> {
        Ok(vec![])
    }
    fn whois(&self, _: SocketAddr) -> Result<WhoIs, TailnetError> {
        (self.answer)()
    }
}

fn whois(id: &str, user: u64, tags: &[&str]) -> WhoIs {
    WhoIs {
        node: WhoIsNode {
            stable_id: id.into(),
            name: format!("{NODE_NAME}."),
            user: Some(user),
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        },
        user_profile: UserProfile {
            id: Some(user),
            login_name: "me@example.com".into(),
            display_name: "Me".into(),
        },
    }
}

/// The harness daemon says it is `nSelf`; the client's whois says `answer`.
fn verified(
    mut options: skyelab_client::Options,
    answer: fn() -> Result<WhoIs, TailnetError>,
) -> skyelab_client::Options {
    // The verifier the app itself ships (`main` builds its options with `client_options`).
    options.verify_peer =
        crate::environment::client_options(Arc::new(ClientTailnet { answer })).verify_peer;
    options
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..50 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn settings_with(pin: Option<&str>) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    if let Some(pin) = pin {
        let mut s = AppSettings::default();
        s.trust_environment_node(NODE_NAME, pin);
        s.save(&path).expect("save");
    }
    (dir, path)
}

fn pinned(path: &Path) -> Option<String> {
    AppSettings::load(path)
        .expect("settings")
        .environment_node(NODE_NAME)
        .map(str::to_owned)
}

/// Switches to the remote and waits for a state containing `want`; then nothing was listed or run, and the
/// client never reported a trusted connection.
fn refused(
    options: skyelab_client::Options,
    remote: &RemoteSide,
    want: &str,
    cx: &mut TestAppContext,
) {
    let (_dir, path) = settings_with(Some("nSelf"));
    let (ws, _local, cx) = local_workspace(Arc::default(), Some(path.clone()), options, cx);
    switch(&ws, Some(NODE_NAME), cx);
    wait_until(cx, want, |cx| state(&ws, cx).contains(want));
    settle(cx);
    assert!(state(&ws, cx).contains(want), "{}", state(&ws, cx));
    assert!(sidebar(&ws, cx)[0].1.is_empty(), "nothing listed");
    let hello = ws.read_with(cx, |ws, cx| {
        ws.remote_env().map(|e| e.read(cx).client().hello())
    });
    assert_eq!(hello, Some(None), "no trusted connection");
    assert!(remote.ran().is_empty());
    assert_eq!(pinned(&path).as_deref(), Some("nSelf"), "pin untouched");
}

const NOT_YOURS: &str = "can't confirm that machine is one of your own devices";

#[gpui::test]
fn your_own_pinned_node_connects(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (_dir, path) = settings_with(Some("nSelf"));
    let options = verified(remote.options(), || Ok(whois("nSelf", ME, &[])));
    let (ws, _local, cx) = local_workspace(Arc::default(), Some(path.clone()), options, cx);
    connect(&ws, &remote, cx);
    assert_eq!(pinned(&path).as_deref(), Some("nSelf"));
}

#[gpui::test]
fn the_pin_is_the_whois_id_not_the_hello(cx: &mut TestAppContext) {
    // The server sends no id at all; the pin still comes from the client's own Tailscale.
    let mut options = test_options().plaintext();
    options.port = nodeless_server();
    let options = verified(options, || Ok(whois("nWhois", ME, &[])));
    let (_dir, path) = settings_with(None);
    let (ws, _local, cx) = local_workspace(Arc::default(), Some(path.clone()), options, cx);
    switch(&ws, Some(NODE_NAME), cx);
    wait_until(cx, "connected", |cx| state(&ws, cx) == "Connected");
    assert_eq!(pinned(&path).as_deref(), Some("nWhois"));
}

#[gpui::test]
fn a_different_node_by_whois_is_the_node_changed_warning(cx: &mut TestAppContext) {
    // The server and whois agree it is nSelf, but this name was pinned to nOld.
    let remote = remote_side(FakeTailnet::default());
    let (_dir, path) = settings_with(Some("nOld"));
    let options = verified(remote.options(), || Ok(whois("nSelf", ME, &[])));
    let (ws, _local, cx) = local_workspace(Arc::default(), Some(path.clone()), options, cx);
    switch(&ws, Some(NODE_NAME), cx);
    wait_until(cx, "the warning", |cx| {
        state(&ws, cx).contains("different machine")
    });
    settle(cx);
    assert!(sidebar(&ws, cx)[0].1.is_empty());
    assert_eq!(pinned(&path).as_deref(), Some("nOld"));
}

#[gpui::test]
fn a_node_claiming_the_pinned_id_is_refused(cx: &mut TestAppContext) {
    // The impostor's hello says nSelf (the pinned id); the client's own Tailscale says it is nImpostor.
    let remote = remote_side(FakeTailnet::default());
    let options = verified(remote.options(), || Ok(whois("nImpostor", ME, &[])));
    refused(options, &remote, "identity doesn't match", cx);
}

#[gpui::test]
fn another_users_device_is_refused(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let options = verified(remote.options(), || Ok(whois("nSelf", ME + 1, &[])));
    refused(options, &remote, NOT_YOURS, cx);
}

#[gpui::test]
fn a_tagged_device_is_refused(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let options = verified(remote.options(), || Ok(whois("nSelf", ME, &["tag:server"])));
    refused(options, &remote, NOT_YOURS, cx);
}

#[gpui::test]
fn a_failed_whois_is_refused(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let options = verified(remote.options(), || {
        Err(TailnetError::Command {
            command: "whois",
            stderr: "no such peer".into(),
        })
    });
    refused(options, &remote, NOT_YOURS, cx);
}

#[gpui::test]
fn a_whois_timeout_is_refused(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let options = verified(remote.options(), || {
        Err(TailnetError::Timeout {
            command: "whois",
            after: Duration::from_secs(5),
        })
    });
    refused(options, &remote, NOT_YOURS, cx);
}

#[gpui::test]
fn without_a_verifier_it_still_connects(cx: &mut TestAppContext) {
    // Mobile has no local Tailscale CLI and passes none: the hello id is pinned, as before.
    let remote = remote_side(FakeTailnet::default());
    let (_dir, path) = settings_with(None);
    let (ws, _local, cx) =
        local_workspace(Arc::default(), Some(path.clone()), remote.options(), cx);
    connect(&ws, &remote, cx);
    assert!(remote.options().verify_peer.is_none());
    assert_eq!(pinned(&path).as_deref(), Some("nSelf"));
}
