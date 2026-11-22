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
        .add_binding(Binding {
            path: dir.clone(),
            account: "acme".into(),
        })
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
