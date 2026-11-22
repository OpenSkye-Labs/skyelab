use super::thread::{SendPrompt, Stop, ThreadView};
use super::thread_model::Item;
use super::threads_test::test_backend;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_core::{Account, Binding};
use std::time::Duration;

fn wait(cx: &mut VisualTestContext, mut done: impl FnMut(&mut VisualTestContext) -> bool) {
    for _ in 0..1000 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out");
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
            t.error
                .as_deref()
                .is_some_and(|e| e.contains("now bound to Beta")),
            "{:?}",
            t.error
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
    wait(cx, |cx| {
        view.read_with(cx, |t, _| t.read_only && t.session.is_none())
    });
    let rows = store
        .lock()
        .expect("lock")
        .list_threads(project)
        .expect("list");
    assert_eq!(rows[0].status, "stopped");
}
