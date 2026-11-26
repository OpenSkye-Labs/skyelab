//! A thread's row ends `stopped` when its tab closes or the app quits (`interrupted` when that cuts a turn off), never
//! `running` with no agent behind it. The agent is the fake claude.

use super::thread::ThreadView;
use super::*;
use gpui::TestAppContext;
use skyelab_core::{Account, Binding};
use std::time::Duration;

pub(super) struct Rig {
    pub(super) home: tempfile::TempDir,
    pub(super) dir: PathBuf,
    pub(super) config: AccountsConfig,
    pub(super) store: SharedStore,
    pub(super) project: skyelab_store::Project,
}

pub(super) fn rig(cx: &mut TestAppContext) -> Rig {
    cx.executor().allow_parking(); // the fake agent is a real process
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
    let home = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(home.path()).expect("canonicalize");
    let dir = root.join("work-a");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let mut config = AccountsConfig::default();
    for id in ["work", "me"] {
        config
            .add_account(Account {
                id: id.into(),
                label: format!("{id} account"),
                config_dir: root.join(format!(".claude-{id}")),
                gh_config_dir: None,
                ssh_agent: false,
                provider: Default::default(),
            })
            .expect("account");
    }
    config
        .add_binding(Binding::new(dir.clone(), "work".into()))
        .expect("bind");
    let store = Store::open_in_memory().expect("store");
    let project = store.create_project("work-a", &dir).expect("project");
    Rig {
        home,
        dir,
        config,
        store: Arc::new(Mutex::new(store)),
        project,
    }
}

pub(super) fn backend(r: &Rig) -> Backend {
    Backend {
        store: r.store.clone(),
        claude_bin: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/fake-claude-delegate.sh"),
    }
}

/// The project's first thread row, if it exists yet.
pub(super) fn row(r: &Rig) -> Option<skyelab_store::Thread> {
    r.store
        .lock()
        .expect("lock")
        .list_threads(r.project.id)
        .expect("threads")
        .into_iter()
        .next()
}

pub(super) use crate::test_wait::wait_until;

#[gpui::test]
fn a_tab_closed_while_starting_leaves_its_thread_stopped(cx: &mut TestAppContext) {
    let r = rig(cx);
    let (config, backend, project) = (r.config.clone(), backend(&r), r.project.clone());
    let (view, cx) =
        cx.add_window_view(|window, cx| ThreadView::new(&project, config, backend, window, cx));
    view.update_in(cx, |v, window, cx| v.submit_remote("hold", window, cx))
        .expect("prompt taken");
    let weak = view.downgrade();
    drop(view);
    // One task at a time, until the launch's background work has written its status: the view hasn't taken the
    // new session yet.
    let mut spins = 0;
    while row(&r).is_none_or(|t| t.status != "running") {
        if !cx.executor().tick() {
            std::thread::sleep(Duration::from_millis(1));
        }
        spins += 1;
        assert!(spins < 100_000, "the launch never wrote its status");
    }
    cx.update(|window, _| window.remove_window());
    wait_until(cx, "the row to say stopped", |_| {
        row(&r).is_some_and(|t| t.status == "stopped")
    });
    assert!(weak.upgrade().is_none(), "the tab is gone");
}

/// Closing the tab of a thread mid-turn ends its agent and leaves the row `interrupted`.
#[gpui::test]
fn a_tab_closed_while_running_leaves_its_thread_interrupted(cx: &mut TestAppContext) {
    let r = rig(cx);
    let (config, backend, project) = (r.config.clone(), backend(&r), r.project.clone());
    let (view, cx) =
        cx.add_window_view(|window, cx| ThreadView::new(&project, config, backend, window, cx));
    view.update_in(cx, |v, window, cx| v.submit_remote("hold", window, cx))
        .expect("prompt taken");
    wait_until(cx, "the agent to run", |cx| {
        view.read_with(cx, |v, _| v.session.is_some())
            && row(&r).is_some_and(|t| t.status == "running")
    });
    drop(view);
    cx.update(|window, _| window.remove_window());
    wait_until(cx, "the row to say interrupted", |_| {
        row(&r).is_some_and(|t| t.status == "interrupted")
    });
}

/// Quitting the app ends every running agent and leaves its row `interrupted` (its turn was cut off), so nothing reads
/// as running once the app is gone, and the turn ends interrupted in the store.
#[gpui::test]
fn quitting_leaves_running_threads_interrupted(cx: &mut TestAppContext) {
    let r = rig(cx);
    let (config, backend, project) = (r.config.clone(), backend(&r), r.project.clone());
    let (view, vcx) =
        cx.add_window_view(|window, cx| ThreadView::new(&project, config, backend, window, cx));
    view.update_in(vcx, |v, window, cx| v.submit_remote("hold", window, cx))
        .expect("prompt taken");
    wait_until(vcx, "the agent to run", |cx| {
        view.read_with(cx, |v, _| v.session.is_some())
            && row(&r).is_some_and(|t| t.status == "running")
    });
    cx.quit();
    let t = row(&r).expect("row");
    assert_eq!(t.status, "interrupted");
    let last = r.store.lock().expect("lock").events_since(t.id, 0).expect("events");
    assert!(
        last.last().is_some_and(|e| e.payload_json.contains("\"result\":\"interrupted\"")),
        "the cut-off turn's end is stored: {:?}",
        last.last()
    );
}
