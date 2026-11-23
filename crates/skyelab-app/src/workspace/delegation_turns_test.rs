//! A follow-up queued behind a running turn keeps the child counted as busy until its own result: the first
//! turn's result must not mark the row idle, or a sibling could take the slot while it still works.

use super::thread::ThreadView;
use super::thread_model::Record;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::Event;
use skyelab_core::{Account, Binding};
use std::time::Duration;

fn wait_until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..1500 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

#[gpui::test]
fn a_queued_follow_up_keeps_the_child_busy_until_its_own_result(cx: &mut TestAppContext) {
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
            provider: Default::default(),
        })
        .expect("account");
    config
        .add_binding(Binding::new(dir.clone(), "work".into()))
        .expect("bind");
    let store = Store::open_in_memory().expect("store");
    let project = store.create_project("work-a", &dir).expect("project");
    let parent = store
        .create_thread(&skyelab_store::NewThread {
            project_id: project.id,
            title: "parent",
            cwd: &dir,
            account_id: "work",
            status: "running",
        })
        .expect("parent")
        .id;
    let store: SharedStore = Arc::new(Mutex::new(store));
    let backend = Backend {
        store: store.clone(),
        claude_bin: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/fake-claude-turns.sh"),
    };
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut v = ThreadView::new(&project, config, backend, window, cx);
        v.parent = Some((parent, "parent".into()));
        v
    });
    let row = |cx: &mut VisualTestContext| {
        let id = view.read_with(cx, |v, _| v.thread.as_ref().map(|t| t.id))?;
        let s = store.lock().expect("lock");
        let results = s
            .events_since(id, 0)
            .expect("events")
            .iter()
            .filter(|e| {
                matches!(
                    Record::from_row(&e.kind, &e.payload_json),
                    Ok(Record::Agent(Event::Result { .. }))
                )
            })
            .count();
        Some((s.thread(id).expect("read").expect("row").status, results))
    };
    view.update_in(cx, |v, window, cx| v.submit_remote("first", window, cx))
        .expect("started");
    wait_until(cx, "the session", |cx| {
        view.read_with(cx, |v, _| v.session.is_some())
    });
    view.update_in(cx, |v, window, cx| v.submit_remote("second", window, cx))
        .expect("queued behind the first turn");
    wait_until(cx, "the first result", |cx| {
        row(cx).is_some_and(|(_, n)| n == 1)
    });
    assert_eq!(
        row(cx).expect("row").0,
        "running",
        "the second turn is still working"
    );
    wait_until(cx, "the second result", |cx| {
        row(cx).is_some_and(|(status, n)| n == 2 && status == "idle")
    });
}
