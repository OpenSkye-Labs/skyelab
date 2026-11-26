//! A remote machine's cached shell: shown at launch while connecting, replaced and saved again by the live stream,
//! and gone from the window and the disk on a refusal, a changed node or Forget.

use super::env_harness::*;
use super::env_linked::ToggleLinkedEnvironment;
use super::env_picker::ForgetEnvironment;
use super::*;
use crate::environment::ShellCacheDir;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_client::{Project, Thread};
use skyelab_core::AppSettings;
use skyelab_daemon::testing::NODE_NAME;
use skyelab_env::cache;
use std::time::Duration;

const CACHED: &str = "cached thread";
const LIVE: &str = "fix the login bug";
/// The node the harness's remote presents.
const NODE: &str = "nSelf";

struct Dirs {
    _tmp: tempfile::TempDir,
    settings: PathBuf,
    cache: PathBuf,
}

/// A settings file pinning [`NODE_NAME`] to `pinned`, and a cache of one project and [`CACHED`] saved from `saved`.
fn dirs(pinned: &str, saved: &str) -> Dirs {
    let tmp = tempfile::tempdir().expect("temp dir");
    let settings = tmp.path().join("settings.toml");
    let mut s = AppSettings::default();
    s.trust_environment_node(NODE_NAME, pinned);
    s.save(&settings).expect("save settings");
    let cache = tmp.path().join("remote-shells");
    write_cache(&cache, saved);
    Dirs {
        _tmp: tmp,
        settings,
        cache,
    }
}

fn write_cache(dir: &Path, node: &str) {
    let project = Project {
        id: 1,
        name: "cached-app".into(),
        path: "/remote/app".into(),
        created_at: 0,
    };
    let thread = Thread {
        id: 99,
        project_id: 1,
        title: CACHED.into(),
        account_id: "work".into(),
        status: "idle".into(),
        ..Thread::default()
    };
    let cached = cache::Cached::new(NODE_NAME, node, vec![project], vec![thread]);
    cache::save(dir, &cached).expect("save cache");
}

fn workspace<'a>(
    remote: &RemoteSide,
    d: &Dirs,
    cx: &'a mut TestAppContext,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    with_options(remote, remote.options(), d, cx)
}

fn with_options<'a>(
    remote: &RemoteSide,
    options: skyelab_client::Options,
    d: &Dirs,
    cx: &'a mut TestAppContext,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    let (ws, _, cx) = local_workspace(remote.tailnet.clone(), Some(d.settings.clone()), options, cx);
    let dir = d.cache.clone();
    cx.update(|_, cx| cx.set_global(ShellCacheDir(dir)));
    (ws, cx)
}

fn threads(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<String> {
    sidebar(ws, cx)
        .into_iter()
        .find(|(t, _)| t == "Threads")
        .map(|(_, r)| r)
        .unwrap_or_default()
}

fn on_disk(d: &Dirs) -> Option<Vec<String>> {
    cache::load(&d.cache, NODE_NAME, Some(NODE))
        .map(|c| c.threads.into_iter().map(|t| t.title).collect())
}

/// A port that takes the connection and never answers it, so it stays being made; the listener holds it.
fn silent() -> (std::net::TcpListener, skyelab_client::Options) {

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let options = skyelab_client::testing::options(port, skyelab_daemon::testing::ca().root());
    (listener, options)
}

/// Lets the save that follows live rows come due.
fn save_due(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(Duration::from_secs(3));
    cx.run_until_parked();
}

#[gpui::test]
fn cached_rows_show_at_launch_while_connecting(cx: &mut TestAppContext) {
    let remote = unserved_remote_side(FakeTailnet::default());
    let d = dirs(NODE, NODE);
    let (_listener, options) = silent();
    let (ws, cx) = with_options(&remote, options, &d, cx);
    switch(&ws, Some(NODE_NAME), cx);
    wait_until(cx, "the cached thread", |cx| {
        threads(&ws, cx).contains(&CACHED.to_owned())
    });
    assert_eq!(state(&ws, cx), "Connecting…");
    assert!(sidebar(&ws, cx)[0].1.contains(&"cached-app".to_owned()));
}

#[gpui::test]
fn cached_rows_of_a_linked_machine_show_while_connecting(cx: &mut TestAppContext) {
    let remote = unserved_remote_side(FakeTailnet::default());
    let d = dirs(NODE, NODE);
    let (_listener, options) = silent();
    let (ws, cx) = with_options(&remote, options, &d, cx);
    let host = NODE_NAME.to_owned();
    ws.update_in(cx, |_, window, cx| {
        window.dispatch_action(Box::new(ToggleLinkedEnvironment { host }), cx)
    });
    wait_until(cx, "the cached linked thread", |cx| {
        raw_sidebar(&ws, cx)
            .iter()
            .any(|(_, rows)| rows.contains(&CACHED.to_owned()))
    });
    assert!(
        raw_sidebar(&ws, cx)
            .iter()
            .any(|(t, r)| t == "Linked machines" && r.iter().any(|r| r.contains("Connecting"))),
        "{:?}",
        raw_sidebar(&ws, cx)
    );
}

#[gpui::test]
fn live_rows_replace_the_cache_and_are_saved(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let d = dirs(NODE, NODE);
    let (ws, cx) = workspace(&remote, &d, cx);
    connect(&ws, &remote, cx);
    wait_until(cx, "the live thread", |cx| {
        threads(&ws, cx) == vec![LIVE.to_owned()]
    });
    save_due(cx);
    wait_until(cx, "the live rows saved", |_| {
        on_disk(&d) == Some(vec![LIVE.to_owned()])
    });

    // A cache read that finishes after live rows came never replaces them.
    write_cache(&d.cache, NODE);
    let env = ws.read_with(cx, |ws, _| ws.remote_env().expect("remote").clone());
    env.update(cx, |e, cx| e.load_cache(cx));
    for _ in 0..20 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(threads(&ws, cx), vec![LIVE.to_owned()]);
}

#[gpui::test]
fn a_refusal_clears_the_window_and_the_cache(cx: &mut TestAppContext) {
    let mut remote = remote_side(FakeTailnet::default());
    let d = dirs(NODE, NODE);
    let (ws, cx) = workspace(&remote, &d, cx);
    connect(&ws, &remote, cx);
    save_due(cx);
    wait_until(cx, "the live rows saved", |_| {
        on_disk(&d) == Some(vec![LIVE.to_owned()])
    });

    remote.stop();
    remote.tailnet.set_peer(Peer::Tagged);
    remote.serve();
    wait_until(cx, "the refusal", |cx| {
        state(&ws, cx) == "That machine didn't accept this device"
    });
    assert!(sidebar(&ws, cx)[0].1.is_empty(), "no projects left");
    assert!(threads(&ws, cx).is_empty(), "no threads left");
    wait_until(cx, "the cache deleted", |_| {
        !cache::file(&d.cache, NODE_NAME).exists()
    });
}

#[gpui::test]
fn a_changed_node_clears_the_cached_rows_and_file(cx: &mut TestAppContext) {
    // Pinned (and cached) as another node than the one that now answers.
    let remote = remote_side(FakeTailnet::default());
    let d = dirs("nOld", "nOld");
    let (ws, cx) = workspace(&remote, &d, cx);
    switch(&ws, Some(NODE_NAME), cx);
    wait_until(cx, "the warning", |cx| {
        state(&ws, cx).contains("different machine")
    });
    assert!(sidebar(&ws, cx)[0].1.is_empty(), "no projects left");
    assert!(threads(&ws, cx).is_empty(), "no threads left");
    wait_until(cx, "the cache deleted", |_| {
        !cache::file(&d.cache, NODE_NAME).exists()
    });
}

#[gpui::test]
fn forget_deletes_the_cache(cx: &mut TestAppContext) {
    let remote = unserved_remote_side(FakeTailnet::default());
    let d = dirs(NODE, NODE);
    let (ws, cx) = workspace(&remote, &d, cx);
    let host = NODE_NAME.to_owned();
    ws.update_in(cx, |_, window, cx| {
        window.dispatch_action(Box::new(ForgetEnvironment { host }), cx)
    });
    wait_until(cx, "the cache deleted", |_| {
        !cache::file(&d.cache, NODE_NAME).exists()
    });
}
