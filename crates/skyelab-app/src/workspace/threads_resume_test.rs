//! Reopening and resuming threads under their bound account. The fake agent is a shell script, so unix only.

use super::*;

/// Runs a thread to its result, stops it, and returns its store row.
fn stopped_thread<'a>(
    f: &Fixture,
    cx: &'a mut TestAppContext,
) -> (Thread, Entity<Workspace>, &'a mut VisualTestContext) {
    let (ws, cx) = open_workspace(f, cx);
    let thread = project_thread(f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    thread.update_in(cx, |t, window, cx| t.stop(&Stop, window, cx));
    wait_until(cx, "the session to stop", |cx| {
        thread.read_with(cx, |t, _| t.session.is_none())
    });
    let row = f
        .store
        .lock()
        .expect("lock")
        .thread(thread.read_with(cx, |t, _| t.thread.as_ref().expect("row").id))
        .expect("read")
        .expect("row");
    assert_eq!(row.agent_session_id.as_deref(), Some("fake-sess-1"));
    (row, ws, cx)
}

/// Reopens `row` like the sidebar does, with the accounts file as it is now.
fn reopen(f: &Fixture, row: &Thread, cx: &mut VisualTestContext) -> Entity<ThreadView> {
    let config = AccountsConfig::load(&f.accounts_path).expect("load");
    let backend = Backend {
        store: f.store.clone(),
        claude_bin: fake_bin(),
    };
    let row = row.clone();
    let view =
        cx.new_window_entity(|window, cx| ThreadView::open(row, config, backend, window, cx));
    wait_until(cx, "the row to load", |cx| {
        view.read_with(cx, |t, _| !t.read_only)
    });
    view
}

/// Sends `text` from a view that isn't the window's root, so keystrokes can't reach it.
fn send_text(view: &Entity<ThreadView>, text: &'static str, cx: &mut VisualTestContext) {
    view.update_in(cx, |t, window, cx| {
        t.prompt.update(cx, |p, cx| p.set_value(text, window, cx));
        t.send_prompt(&super::thread::SendPrompt, window, cx);
    });
    cx.run_until_parked();
}

fn env_reports(store: &SharedStore, thread_id: i64) -> Vec<serde_json::Value> {
    stored_records(store, thread_id)
        .into_iter()
        .filter_map(|r| match r {
            Record::Agent(skyelab_agent::Event::Unknown(v)) if v["type"] == "fake_env" => Some(v),
            _ => None,
        })
        .collect()
}

/// Rebinds the fixture's folder: to `to`, or unbound when `None`.
fn rebind(f: &Fixture, to: Option<&str>) {
    let mut config = AccountsConfig::load(&f.accounts_path).expect("load");
    config
        .add_account(Account {
            id: "other".into(),
            label: "Other Co".into(),
            config_dir: f.account_dir.with_file_name(".claude-other"),
            gh_config_dir: None,
        })
        .expect("add account");
    config.remove_binding(&f.project_dir).expect("unbind");
    if let Some(to) = to {
        config
            .add_binding(Binding::new(f.project_dir.clone(), to.into()))
            .expect("bind");
    }
    config.save(&f.accounts_path).expect("save");
}

#[gpui::test]
fn reopened_thread_resumes_under_its_account(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (row, _ws, cx) = stopped_thread(&f, cx);
    let view = reopen(&f, &row, cx);
    send_text(&view, "again", cx);
    wait_until(cx, "the resumed agent's env report", |_| {
        env_reports(&f.store, row.id).len() == 2
    });
    let resumed = &env_reports(&f.store, row.id)[1];
    let args = resumed["args"].as_str().expect("args");
    assert!(args.contains("--resume fake-sess-1"), "{args}");
    assert_eq!(
        resumed["config_dir"],
        f.account_dir.to_str().expect("utf-8")
    );
    view.read_with(cx, |t, _| {
        assert!(t.session.is_some(), "the resumed session runs");
        assert_eq!(t.resume_refused, None);
        assert_eq!(t.thread.as_ref().map(|t| t.id), Some(row.id), "same row");
    });
    let rows = f.store.lock().expect("lock").list_threads(row.project_id);
    assert_eq!(rows.expect("list").len(), 1, "resuming adds no thread row");
}

fn assert_refused(f: &Fixture, row: &Thread, cx: &mut VisualTestContext, says: &str) {
    let config = AccountsConfig::load(&f.accounts_path).expect("load");
    let backend = Backend {
        store: f.store.clone(),
        claude_bin: fake_bin(),
    };
    let row = row.clone();
    // Its own window, with the view as root, so the refusal is drawn and its button can be found.
    let (view, cx) = std::ops::DerefMut::deref_mut(cx)
        .add_window_view(|window, cx| ThreadView::open(row.clone(), config, backend, window, cx));
    wait_until(cx, "the row to load", |cx| {
        view.read_with(cx, |t, _| !t.read_only)
    });
    send_text(&view, "again", cx);
    view.read_with(cx, |t, _| {
        let why = t.resume_refused.clone().expect("resume refused");
        assert!(why.contains(says), "{why}");
        assert!(t.session.is_none(), "nothing started");
        assert!(!t.starting);
    });
    std::thread::sleep(Duration::from_millis(300));
    cx.run_until_parked();
    let thread_id = view.read_with(cx, |t, _| t.thread.as_ref().map(|t| t.id));
    assert_eq!(
        env_reports(&f.store, thread_id.expect("row")).len(),
        1,
        "no second agent ran"
    );
    assert!(
        cx.debug_bounds("new-thread-cta").is_some(),
        "offers a new thread"
    );
}

#[gpui::test]
fn resume_refuses_another_account(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (row, _ws, cx) = stopped_thread(&f, cx);
    rebind(&f, Some("other"));
    assert_refused(&f, &row, cx, "now bound to Other Co");
}

#[gpui::test]
fn resume_refuses_unbound_folder(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (row, _ws, cx) = stopped_thread(&f, cx);
    rebind(&f, None);
    assert_refused(&f, &row, cx, "isn't bound to acme");
}
