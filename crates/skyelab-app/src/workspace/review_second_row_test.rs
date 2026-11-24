use super::thread::{SendPrompt, Stop, ThreadView};
use super::thread_model::Item;
use super::threads_test::test_backend;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_core::{Account, Binding};
use std::time::Duration;

fn wait(cx: &mut VisualTestContext, done: impl FnMut(&mut VisualTestContext) -> bool) {
    crate::test_wait::wait_until(cx, "the fake agent", done);
}

fn send(view: &Entity<ThreadView>, text: &'static str, cx: &mut VisualTestContext) {
    view.update_in(cx, |t, window, cx| {
        t.prompt.update(cx, |p, cx| p.set_value(text, window, cx));
        t.send_prompt(&SendPrompt, window, cx);
    });
}

#[cfg(unix)]
#[gpui::test]
fn sending_after_stop_keeps_one_thread_row(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
    let home = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(home.path()).expect("canonicalize");
    let dir = root.join("demo");
    std::fs::create_dir_all(&dir).expect("dir");
    let mut config = AccountsConfig::default();
    config
        .add_account(Account {
            id: "acme".into(),
            label: "Acme".into(),
            config_dir: root.join(".claude-acme"),
            provider: Default::default(),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("account");
    config
        .add_binding(Binding::new(dir.clone(), "acme".into()))
        .expect("bind");
    let backend = test_backend();
    let store = backend.store.clone();
    let project = store
        .lock()
        .expect("lock")
        .create_project("demo", &dir)
        .expect("project");
    let p = project.clone();
    let (view, cx) =
        cx.add_window_view(move |window, cx| ThreadView::new(&p, config, backend, window, cx));
    send(&view, "one", cx);
    wait(cx, |cx| {
        view.read_with(cx, |t, _| {
            t.items.iter().any(|i| matches!(i, Item::Done { .. }))
        })
    });
    view.update_in(cx, |t, window, cx| t.stop(&Stop, window, cx));
    wait(cx, |cx| view.read_with(cx, |t, _| t.session.is_none()));
    send(&view, "two", cx);
    wait(cx, |cx| view.read_with(cx, |t, _| !t.starting));
    let rows = store
        .lock()
        .expect("lock")
        .list_threads(project.id)
        .expect("list");
    assert_eq!(rows.len(), 1, "SECOND_THREAD_ROW");
}

fn bound_view(
    cx: &mut TestAppContext,
) -> (
    Entity<ThreadView>,
    &mut VisualTestContext,
    SharedStore,
    i64,
    AccountsConfig,
    tempfile::TempDir,
) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
    let home = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(home.path()).expect("canonicalize");
    let dir = root.join("demo");
    std::fs::create_dir_all(&dir).expect("dir");
    let mut config = AccountsConfig::default();
    for (id, label) in [("acme", "Acme"), ("beta", "Beta")] {
        config
            .add_account(Account {
                id: id.into(),
                label: label.into(),
                config_dir: root.join(format!(".claude-{id}")),
                provider: Default::default(),
                gh_config_dir: None,
                ssh_agent: false,
            })
            .expect("account");
    }
    config
        .add_binding(Binding::new(dir.clone(), "acme".into()))
        .expect("bind");
    let backend = test_backend();
    let store = backend.store.clone();
    let project = store
        .lock()
        .expect("lock")
        .create_project("demo", &dir)
        .expect("project");
    let (p, c) = (project.clone(), config.clone());
    let (view, cx) =
        cx.add_window_view(move |window, cx| ThreadView::new(&p, c, backend, window, cx));
    (view, cx, store, project.id, config, home)
}

#[cfg(unix)]
#[gpui::test]
fn a_rebound_folder_refuses_follow_ups_to_the_old_account(cx: &mut TestAppContext) {
    let (view, cx, store, _, mut config, _home) = bound_view(cx);
    send(&view, "one", cx);
    wait(cx, |cx| {
        view.read_with(cx, |t, _| {
            t.items.iter().any(|i| matches!(i, Item::Done { .. }))
        })
    });
    let dir = config.bindings[0].path.clone();
    config.remove_binding(&dir).expect("unbind");
    config
        .add_binding(Binding::new(dir, "beta".into()))
        .expect("rebind");
    view.update(cx, |t, cx| t.set_config(config, cx));
    send(&view, "two", cx);
    cx.run_until_parked();
    let id = view.read_with(cx, |t, _| {
        assert!(
            t.resume_refused
                .as_deref()
                .is_some_and(|e| e.contains("now bound to Beta")),
            "{:?}",
            t.resume_refused
        );
        t.thread.as_ref().expect("row").id
    });
    let prompts = store
        .lock()
        .expect("lock")
        .events_since(id, 0)
        .expect("events")
        .into_iter()
        .filter(|e| e.kind == "prompt")
        .count();
    assert_eq!(
        prompts, 1,
        "the second prompt never reached the old account's agent"
    );
}

#[cfg(unix)]
#[gpui::test]
fn stop_while_starting_stops_the_new_agent(cx: &mut TestAppContext) {
    let (view, cx, store, project, _, _home) = bound_view(cx);
    send(&view, "one", cx);
    view.update_in(cx, |t, window, cx| {
        assert!(t.starting, "precondition: still starting");
        t.stop(&Stop, window, cx)
    });
    // Read-only or resumable depends on whether claude's Init got out before the kill; either way it is stopped.
    wait(cx, |cx| {
        view.read_with(cx, |t, _| !t.starting && t.session.is_none())
            && store
                .lock()
                .expect("lock")
                .list_threads(project)
                .expect("list")
                .first()
                .is_some_and(|r| r.status == "stopped")
    });
}

/// After a Stop, the old session's event stream can stay open (a tool that left claude's process
/// group still holds stdout). When it finally closes, it must not end a session resumed meanwhile.
#[cfg(unix)]
#[gpui::test]
fn a_late_end_of_the_stopped_session_leaves_the_resumed_one_running(cx: &mut TestAppContext) {
    let (_, cx, store, project_id, config, _home) = bound_view(cx);
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lingering-claude.sh");
    let project = store
        .lock()
        .expect("lock")
        .project(project_id)
        .expect("read")
        .expect("project");
    let backend = Backend {
        store,
        claude_bin: bin,
    };
    let view =
        cx.new_window_entity(|window, cx| ThreadView::new(&project, config, backend, window, cx));
    send(&view, "one", cx);
    wait(cx, |cx| {
        view.read_with(cx, |t, _| {
            t.items.iter().any(|i| matches!(i, Item::Done { .. }))
        })
    });
    view.update_in(cx, |t, window, cx| t.stop(&Stop, window, cx));
    wait(cx, |cx| view.read_with(cx, |t, _| t.session.is_none()));
    send(&view, "two", cx); // resumes while the stopped session's stdout is still open
    wait(cx, |cx| view.read_with(cx, |t, _| t.session.is_some()));
    let resumed = view
        .read_with(cx, |t, _| t.session.clone())
        .expect("resumed session");
    for _ in 0..40 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(100)); // past lingering-claude.sh's 3 s
    }
    let current = view.read_with(cx, |t, _| t.session.clone());
    assert!(
        current.is_some_and(|s| Arc::ptr_eq(&s, &resumed)),
        "the stopped session's late end dropped the resumed one"
    );
    let exited = resumed.lock().expect("lock").try_wait().expect("try_wait");
    assert!(exited.is_none(), "the resumed agent was killed");
}
