//! Test harness for the environment switcher: a "remote" SkyeLab (an in-process daemon over a temp-file store, a
//! fake tailnet that admits us) and a local workspace with its own empty store. No agent runs on either side.

use super::*;
use gpui::{TestAppContext, VisualTestContext};
pub(super) use skyelab_daemon::testing::{Env as RemoteSide, FakeTailnet, ME, Peer, Ran, node};
use skyelab_daemon::testing::{NODE_NAME, ca};
use std::time::Duration;

/// The other machine (the shared harness `Env`): one project "app" at /remote/app with one thread on account
/// "work", served on a fixed loopback port.
pub(super) fn remote_side(tailnet: FakeTailnet) -> RemoteSide {
    let mut remote = unserved_remote_side(tailnet);
    remote.serve();
    remote
}

/// [`remote_side`] before its daemon starts.
pub(super) fn unserved_remote_side(tailnet: FakeTailnet) -> RemoteSide {
    RemoteSide::with(tailnet, "/remote/app", "fix the login bug")
}

/// What the switcher tests do with the other machine beyond the shared harness.
pub(super) trait RemoteSideExt {
    /// Stores agent text events on the thread. With `push`, pushes only the last one, so a client must fill the rest.
    fn store_texts(&self, thread_id: i64, texts: &[&str], push: bool);
    fn options(&self) -> skyelab_client::Options;
}

impl RemoteSideExt for RemoteSide {
    fn store_texts(&self, thread_id: i64, texts: &[&str], push: bool) {
        let mut last = None;
        for text in texts {
            let record = super::thread_model::Record::Agent(skyelab_agent::Event::Assistant {
                content: vec![skyelab_agent::Block::Text {
                    text: (*text).to_owned(),
                }],
            });
            let (kind, payload) = record.to_row();
            last = Some(self.append(thread_id, kind, &payload));
        }
        if push && let (Some(last), Some(daemon)) = (last, &self.daemon) {
            daemon.push_event(last);
        }
    }

    fn options(&self) -> skyelab_client::Options {
        skyelab_client::testing::options(self.port, ca().root())
    }
}

pub(super) fn setup(cx: &mut TestAppContext) {
    // The client and daemon threads wake the UI.
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
}

/// A local workspace with an empty store of its own, wired to `tailnet` for the picker.
pub(super) fn local_workspace(
    tailnet: Arc<FakeTailnet>,
    settings: Option<PathBuf>,
    options: skyelab_client::Options,
    cx: &mut TestAppContext,
) -> (Entity<Workspace>, SharedStore, &mut VisualTestContext) {
    setup(cx);
    let store: SharedStore = Arc::new(Mutex::new(Store::open_in_memory().expect("store")));
    let backend = Backend {
        store: store.clone(),
        claude_bin: Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh"),
    };
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let mut ws = Workspace::new(
            std::env::temp_dir(),
            None,
            AccountsConfig::default(),
            None,
            backend,
            window,
            cx,
        );
        ws.set_environments(tailnet, settings, options, window, cx);
        ws
    });
    ws.update_in(cx, |ws, window, cx| ws.focus.clone().focus(window, cx));
    (ws, store, cx)
}

/// Pumps the app until `done`, giving the client and daemon threads real time.
pub(super) fn wait_until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..800 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

pub(super) fn switch(ws: &Entity<Workspace>, host: Option<&str>, cx: &mut VisualTestContext) {
    let action = env_mode::SwitchEnvironment {
        host: host.map(str::to_owned),
    };
    ws.update_in(cx, |ws, window, cx| {
        ws.switch_environment(&action, window, cx)
    });
}

pub(super) fn state(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> String {
    ws.read_with(cx, |ws, cx| {
        ws.remote_env()
            .map(|e| e.read(cx).state.label().to_string())
            .unwrap_or_default()
    })
}

/// The sidebar's sections as (title, row labels).
pub(super) fn sidebar(
    ws: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Vec<(String, Vec<String>)> {
    ws.read_with(cx, |ws, cx| {
        ws.sidebar
            .read(cx)
            .sections
            .iter()
            .map(|s| {
                (
                    s.title.to_string(),
                    s.items.iter().map(|i| i.label.to_string()).collect(),
                )
            })
            .collect()
    })
}

/// Clicks the first sidebar row labelled `label` (dispatches its action on the workspace).
pub(super) fn click_row(ws: &Entity<Workspace>, label: &str, cx: &mut VisualTestContext) {
    let action = ws.read_with(cx, |ws, cx| {
        ws.sidebar
            .read(cx)
            .sections
            .iter()
            .flat_map(|s| &s.items)
            .find(|i| i.label == label)
            .map(|i| i.action.boxed_clone())
            .unwrap_or_else(|| panic!("no sidebar row {label}"))
    });
    ws.update_in(cx, |_, window, cx| window.dispatch_action(action, cx));
}

/// Connected to `remote` and showing its projects.
pub(super) fn connect(ws: &Entity<Workspace>, remote: &RemoteSide, cx: &mut VisualTestContext) {
    let _ = remote;
    switch(ws, Some(NODE_NAME), cx);
    wait_until(cx, "connected", |cx| state(ws, cx) == "Connected");
    wait_until(cx, "remote projects", |cx| {
        sidebar(ws, cx)
            .first()
            .is_some_and(|(_, rows)| rows == &["app".to_owned()])
    });
}

/// A client that trusts the test CA and dials this node's MagicDNS name on loopback (TLS still checks the name).
pub(super) fn test_options() -> skyelab_client::Options {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca().root()).expect("test CA root");
    skyelab_client::Options::default()
        .allow_loopback()
        .trust_roots(roots)
        .resolve_to(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST))
}
