use super::thread::{SendPrompt, Stop, ThreadView};
use super::thread_model::Item;
use super::*;
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use skyelab_core::{Account, Binding};
use skyelab_store::Store;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn wait_until(
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

fn send(view: &Entity<ThreadView>, text: &'static str, cx: &mut VisualTestContext) {
    view.update_in(cx, |t, window, cx| {
        t.prompt.update(cx, |p, cx| p.set_value(text, window, cx));
        t.send_prompt(&SendPrompt, window, cx);
    });
    cx.run_until_parked();
}

/// Thread tabs stay open across project switches, so the refusal's button must start the new
/// thread in the refused thread's folder, not the sidebar's.
#[cfg(unix)]
#[gpui::test]
fn start_a_new_thread_opens_in_the_refused_threads_folder(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
    let home = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(home.path()).expect("canonicalize");
    let first = root.join("demo");
    let second = root.join("second");
    std::fs::create_dir_all(&first).expect("first dir");
    std::fs::create_dir_all(&second).expect("second dir");
    let accounts_path = root.join("accounts.toml");
    let mut config = AccountsConfig::default();
    for (id, label) in [("acme", "Acme Team"), ("other", "Other Co")] {
        config
            .add_account(Account {
                id: id.into(),
                label: label.into(),
                config_dir: root.join(format!(".claude-{id}")),
                provider: Default::default(),
                gh_config_dir: None,
            })
            .expect("add account");
    }
    for dir in [&first, &second] {
        config
            .add_binding(Binding::new(dir.clone(), "acme".into()))
            .expect("bind");
    }
    config.save(&accounts_path).expect("save");

    let backend = Backend {
        store: Arc::new(Mutex::new(Store::open_in_memory().expect("store"))),
        claude_bin: Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh"),
    };
    let loaded = AccountsConfig::load(&accounts_path).expect("load");
    let (path, folder) = (accounts_path.clone(), root.clone());
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let ws = Workspace::new(folder, Some(path), loaded, None, backend, window, cx);
        window.focus(&ws.focus, cx);
        ws
    });
    cx.run_until_parked();
    for (n, dir) in [&first, &second].into_iter().enumerate() {
        let dir = dir.clone();
        ws.update(cx, |ws, cx| ws.add_project(dir, cx));
        wait_until(cx, "project stored", |cx| {
            ws.read_with(cx, |ws, _| ws.projects.len() == n + 1)
        });
    }
    let id_of = |ws: &Workspace, dir: &Path| {
        ws.projects
            .iter()
            .find(|p| p.path == dir)
            .map(|p| p.id)
            .expect("project")
    };

    let first_id = ws.read_with(cx, |ws, _| id_of(ws, &first));
    cx.dispatch_action(SelectProject { id: first_id });
    cx.run_until_parked();
    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    let thread = ws
        .read_with(cx, |ws, _| ws.open_threads.last().and_then(|t| t.upgrade()))
        .expect("thread tab");
    send(&thread, "hi", cx);
    wait_until(cx, "the fake's result", |cx| {
        thread.read_with(cx, |t, _| {
            t.items.iter().any(|i| matches!(i, Item::Done { .. }))
        })
    });
    thread.update_in(cx, |t, window, cx| t.stop(&Stop, window, cx));
    wait_until(cx, "the stop", |cx| {
        thread.read_with(cx, |t, _| t.session.is_none() && !t.read_only)
    });

    let second_id = ws.read_with(cx, |ws, _| id_of(ws, &second));
    cx.dispatch_action(SelectProject { id: second_id });
    cx.run_until_parked();

    let mut config = AccountsConfig::load(&accounts_path).expect("load");
    config.remove_binding(&first).expect("unbind");
    config
        .add_binding(Binding::new(first.clone(), "other".into()))
        .expect("rebind");
    config.save(&accounts_path).expect("save");
    ws.update(cx, |ws, cx| {
        ws.reload_accounts();
        ws.push_config(cx);
    });
    cx.run_until_parked();

    send(&thread, "again", cx);
    thread.read_with(cx, |t, _| {
        assert!(t.resume_refused.is_some(), "precondition: resume refused")
    });
    let before = ws.read_with(cx, |ws, _| ws.open_threads.len());
    let cta = cx
        .debug_bounds("new-thread-cta")
        .expect("precondition: refusal button drawn");
    cx.simulate_click(cta.center(), Modifiers::none());
    cx.run_until_parked();
    let opened = ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.open_threads.len(),
            before + 1,
            "precondition: the button opened a tab"
        );
        ws.open_threads
            .last()
            .and_then(|t| t.upgrade())
            .expect("new tab")
    });
    let cwd = opened.read_with(cx, |t, _| t.cwd.clone());
    assert_eq!(
        cwd, first,
        "Start a new thread opened in another project's folder"
    );
}
