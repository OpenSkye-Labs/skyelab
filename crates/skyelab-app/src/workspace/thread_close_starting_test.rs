//! Audit #6: a tab closed while its launch is still in progress leaves its row `stopped`, never `running` with
//! no agent behind it. The agent is the fake claude.

use super::thread::ThreadView;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
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

/// The project's only thread row, if it exists yet.
pub(super) fn row(r: &Rig) -> Option<skyelab_store::Thread> {
    r.store
        .lock()
        .expect("lock")
        .list_threads(r.project.id)
        .expect("threads")
        .into_iter()
        .next()
}

pub(super) fn wait_until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..1000 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

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
