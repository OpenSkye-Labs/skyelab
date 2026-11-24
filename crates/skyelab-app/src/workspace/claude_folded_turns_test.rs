//! Claude can fold a message sent mid-turn into the running turn and end both with one `result`. Its
//! `user_message_uuids` names the prompts it answered, so the row goes idle then; a background "wake" result that
//! answers none of our prompts ends no turn.

use super::thread::ThreadView;
use super::thread_model::Record;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::Event;
use skyelab_core::{Account, Binding, Provider};
use std::time::Duration;

pub(super) fn wait_until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let until = std::time::Instant::now() + crate::test_wait::DEADLINE;
    while std::time::Instant::now() < until {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
        // The session's exit watch runs on the executor's clock, which only moves when a test moves it.
        cx.executor().advance_clock(Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

pub(super) struct One {
    pub _home: tempfile::TempDir,
    pub dir: PathBuf,
    pub store: SharedStore,
    pub view: Entity<ThreadView>,
}

/// One thread tab on a folder bound to a `provider` account, running `bin`.
pub(super) fn one_view<'a>(
    cx: &'a mut TestAppContext,
    provider: Provider,
    bin: PathBuf,
    marker: Option<&str>,
) -> (One, &'a mut VisualTestContext) {
    cx.executor().allow_parking(); // the fake agent is a real process
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
        cx.set_global(CodexBin(bin.clone()));
    });
    let home = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(home.path()).expect("canonicalize");
    let dir = root.join("proj");
    std::fs::create_dir_all(&dir).expect("mkdir");
    if let Some(m) = marker {
        std::fs::write(dir.join(m), "").expect("marker");
    }
    let mut config = AccountsConfig::default();
    config
        .add_account(Account {
            id: "work".into(),
            label: "work account".into(),
            config_dir: root.join(".home-work"),
            gh_config_dir: None,
            ssh_agent: false,
            provider,
        })
        .expect("account");
    config
        .add_binding(Binding::new(dir.clone(), "work".into()))
        .expect("bind");
    let store = Store::open_in_memory().expect("store");
    let project = store.create_project("proj", &dir).expect("project");
    let store: SharedStore = Arc::new(Mutex::new(store));
    let backend = Backend {
        store: store.clone(),
        claude_bin: bin,
    };
    // Under gpui-component's Root, as in the app, so dialogs (an approval's View all) can open.
    let mut made = None;
    let window = cx.add_window(|window, cx| {
        let view = cx.new(|cx| ThreadView::new(&project, config, backend, window, cx));
        made = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let view = made.expect("the thread view was built");
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    (
        One {
            _home: home,
            dir,
            store,
            view,
        },
        cx,
    )
}

/// The row's status and how many results it stored.
pub(super) fn row(one: &One, cx: &mut VisualTestContext) -> Option<(String, usize)> {
    let id = one
        .view
        .read_with(cx, |v, _| v.thread.as_ref().map(|t| t.id))?;
    let s = one.store.lock().expect("lock");
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
}

#[gpui::test]
fn one_result_for_a_folded_follow_up_leaves_the_row_idle(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude-fold.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("first", window, cx))
        .expect("started");
    wait_until(cx, "the session", |cx| {
        one.view.read_with(cx, |v, _| v.session.is_some())
    });
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("second", window, cx))
        .expect("sent mid-turn");
    wait_until(cx, "idle after the one result", |cx| {
        row(&one, cx).is_some_and(|(status, n)| n == 1 && status == "idle")
    });

    // A wake result answers none of our prompts: the third prompt's turn is still running after it.
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("third", window, cx))
        .expect("third");
    wait_until(cx, "the wake result", |cx| {
        row(&one, cx).is_some_and(|(_, n)| n == 2)
    });
    assert_eq!(row(&one, cx).expect("row").0, "running");
    std::fs::write(one.dir.join(".fake-go"), "").expect("let the third prompt's turn answer");
    wait_until(cx, "the third prompt's result", |cx| {
        row(&one, cx).is_some_and(|(status, n)| n == 3 && status == "idle")
    });
}
