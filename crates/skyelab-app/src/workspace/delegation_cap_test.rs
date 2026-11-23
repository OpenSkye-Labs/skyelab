//! The running-children cap holds on every start and resume of a delegated child, whatever sent the prompt.
//! A Send from another device, a delegated send and the user's own Send all reach `send_prompt`, which
//! `submit_remote` drives here. The agent is the fake claude.

use super::thread::ThreadView;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_core::delegation::MAX_CHILDREN;
use skyelab_core::{Account, Binding};
use std::time::Duration;

struct Rig {
    _home: tempfile::TempDir,
    config: AccountsConfig,
    store: SharedStore,
    project: skyelab_store::Project,
    parent: i64,
}

fn rig(cx: &mut TestAppContext) -> Rig {
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
    config
        .add_account(Account {
            id: "work".into(),
            label: "work account".into(),
            config_dir: root.join(".claude-work"),
            gh_config_dir: None,
            ssh_agent: false,
            provider: Default::default(),
        })
        .expect("account");
    config
        .add_binding(Binding::new(dir.clone(), "work".into()))
        .expect("bind");
    let store = Store::open_in_memory().expect("store");
    let project = store.create_project("work-a", &dir).expect("project");
    let parent = thread_row(&store, &project, "running", None);
    Rig {
        _home: home,
        config,
        store: Arc::new(Mutex::new(store)),
        project,
        parent,
    }
}

fn thread_row(
    store: &Store,
    project: &skyelab_store::Project,
    status: &str,
    parent: Option<i64>,
) -> i64 {
    let t = store
        .create_thread(&skyelab_store::NewThread {
            project_id: project.id,
            title: "a thread",
            cwd: &project.path,
            account_id: "work",
            status,
        })
        .expect("thread");
    store.set_thread_parent(t.id, parent).expect("parent");
    t.id
}

/// `n` siblings under the parent that are working on a turn.
fn busy_siblings(r: &Rig, n: usize) {
    let store = r.store.lock().expect("lock");
    for _ in 0..n {
        thread_row(&store, &r.project, "running", Some(r.parent));
    }
}

fn backend(r: &Rig) -> Backend {
    Backend {
        store: r.store.clone(),
        claude_bin: Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh"),
    }
}

fn status(r: &Rig, id: i64) -> String {
    r.store
        .lock()
        .expect("lock")
        .thread(id)
        .expect("read")
        .expect("row")
        .status
}

fn prompts(r: &Rig, id: i64) -> usize {
    r.store
        .lock()
        .expect("lock")
        .events_since(id, 0)
        .expect("events")
        .iter()
        .filter(|e| e.kind == "prompt")
        .count()
}

use crate::test_wait::wait_until;

/// Waits for whatever the send started to settle, then gives a wrongly started agent time to show up.
fn settle(view: &Entity<ThreadView>, cx: &mut VisualTestContext) {
    wait_until(cx, "the send to settle", |cx| {
        view.read_with(cx, |v, _| !v.starting)
    });
    for _ in 0..30 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn error(view: &Entity<ThreadView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |v, _| v.error.clone().map(|e| e.to_string()))
        .unwrap_or_default()
}

#[gpui::test]
fn a_send_to_a_stopped_child_does_not_resume_it_past_the_cap(cx: &mut TestAppContext) {
    let r = rig(cx);
    busy_siblings(&r, MAX_CHILDREN);
    let child = {
        let store = r.store.lock().expect("lock");
        let id = thread_row(&store, &r.project, "stopped", Some(r.parent));
        store
            .set_agent_session_id(id, Some("sess-1"))
            .expect("session");
        store.thread(id).expect("read").expect("row")
    };
    let (config, backend) = (r.config.clone(), backend(&r));
    let (view, cx) = cx
        .add_window_view(|window, cx| ThreadView::open(child.clone(), config, backend, window, cx));
    wait_until(cx, "the thread to load as resumable", |cx| {
        view.read_with(cx, |v, _| !v.read_only)
    });
    let _ = view.update_in(cx, |v, window, cx| {
        v.submit_remote("keep going", window, cx)
    });
    settle(&view, cx);
    assert_eq!(
        status(&r, child.id),
        "stopped",
        "the child must not run again"
    );
    assert_eq!(prompts(&r, child.id), 0, "no prompt stored");
    assert!(view.read_with(cx, |v, _| v.session.is_none()));
    assert!(
        error(&view, cx).contains("running children"),
        "{}",
        error(&view, cx)
    );
}

#[gpui::test]
fn a_new_child_does_not_start_past_the_cap(cx: &mut TestAppContext) {
    let r = rig(cx);
    busy_siblings(&r, MAX_CHILDREN);
    let (config, backend, project, parent) =
        (r.config.clone(), backend(&r), r.project.clone(), r.parent);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut v = ThreadView::new(&project, config, backend, window, cx);
        v.parent = Some((parent, "parent".into()));
        v
    });
    let _ = view.update_in(cx, |v, window, cx| v.submit_remote("one more", window, cx));
    settle(&view, cx);
    let children = r
        .store
        .lock()
        .expect("lock")
        .child_threads(r.parent)
        .expect("children");
    assert_eq!(children.len(), MAX_CHILDREN, "no fifth child row");
    assert!(view.read_with(cx, |v, _| v.session.is_none()));
    assert!(
        error(&view, cx).contains("running children"),
        "{}",
        error(&view, cx)
    );
}

#[gpui::test]
fn a_follow_up_to_an_idle_child_does_not_pass_the_cap(cx: &mut TestAppContext) {
    let r = rig(cx);
    busy_siblings(&r, MAX_CHILDREN - 1);
    let (config, backend, project, parent) =
        (r.config.clone(), backend(&r), r.project.clone(), r.parent);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut v = ThreadView::new(&project, config, backend, window, cx);
        v.parent = Some((parent, "parent".into()));
        v
    });
    view.update_in(cx, |v, window, cx| v.submit_remote("first", window, cx))
        .expect("the fourth child may start");
    let mut id = None;
    wait_until(cx, "the child to finish its turn", |cx| {
        id = view.read_with(cx, |v, _| v.thread.as_ref().map(|t| t.id));
        id.is_some_and(|id| status(&r, id) == "idle")
    });
    let id = id.expect("waited");
    // Another child got the free slot meanwhile.
    busy_siblings(&r, 1);
    let _ = view.update_in(cx, |v, window, cx| v.submit_remote("again", window, cx));
    settle(&view, cx);
    assert_eq!(status(&r, id), "idle", "the child must stay idle");
    assert_eq!(prompts(&r, id), 1, "the second prompt isn't stored");
    assert!(
        error(&view, cx).contains("running children"),
        "{}",
        error(&view, cx)
    );
}
