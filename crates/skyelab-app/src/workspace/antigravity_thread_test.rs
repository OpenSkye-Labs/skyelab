//! Acceptance: a project bound to an Antigravity account runs threads with `AntigravityBin` under `GEMINI_HOME`,
//! lists Antigravity models/efforts (including Claude-named models) in the composer picker, and supports handoff.
#![cfg(unix)]

use super::thread::ThreadView;
use super::thread_extras::handoff::{ContinueInAntigravity, ContinueInClaude, HandOffThread};
use super::thread_model::{Item, Record};
use super::*;
use crate::test_wait::wait_until;
use gpui::TestAppContext;
use skyelab_core::{Account, Binding, Provider};
use skyelab_store::Store;
use std::sync::{Arc, Mutex};

fn fake_antigravity() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-antigravity.sh")
}

#[gpui::test]
fn antigravity_thread_streams_lists_models_and_hands_off(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
        cx.set_global(AntigravityBin(fake_antigravity()));
    });
    let home = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(home.path()).expect("canonicalize");
    let project = root.join("demo");
    std::fs::create_dir_all(&project).expect("project dir");
    let gemini_home = root.join(".antigravity-work");
    std::fs::create_dir_all(&gemini_home).expect("gemini_home");
    std::fs::write(gemini_home.join("fake-login"), "me@example.com\n").expect("login");

    let accounts_path = root.join("accounts.toml");
    let mut config = AccountsConfig::default();
    config
        .add_account(Account {
            id: "agy".into(),
            label: "Antigravity".into(),
            provider: Provider::Antigravity,
            config_dir: gemini_home.clone(),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("add agy");
    config
        .add_account(Account {
            id: "acme".into(),
            label: "Acme".into(),
            provider: Provider::Claude,
            config_dir: root.join(".claude-acme"),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("add claude");
    config
        .add_binding(Binding::new(project.clone(), "agy".into()))
        .expect("bind");
    config.save(&accounts_path).expect("save");

    let store: SharedStore = Arc::new(Mutex::new(Store::open_in_memory().expect("store")));
    let backend = Backend {
        store: store.clone(),
        claude_bin: root.join("no-such-claude"),
    };
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let ws = Workspace::new(
            root.clone(),
            Some(accounts_path.clone()),
            config.clone(),
            None,
            backend,
            window,
            cx,
        );
        window.focus(&ws.focus, cx);
        ws
    });
    ws.update(cx, |ws, cx| ws.add_project(project.clone(), cx));
    wait_until(cx, "project stored", |cx| {
        ws.read_with(cx, |ws, _| !ws.projects.is_empty())
    });
    let id = ws.read_with(cx, |ws, _| ws.projects[0].id);
    cx.dispatch_action(SelectProject { id });
    cx.run_until_parked();
    cx.dispatch_action(NewThread);
    cx.run_until_parked();

    let thread: Entity<ThreadView> = ws
        .read_with(cx, |ws, _| ws.open_threads.last().cloned())
        .expect("thread tab");

    // Composer picker lists Antigravity models (including claude-sonnet-4-5) and efforts.
    thread.update_in(cx, |t, window, cx| t.toggle_picker(window, cx));
    wait_until(cx, "antigravity models listed", |cx| {
        thread.read_with(cx, |t, _| {
            matches!(&t.composer.antigravity_models, Some(Ok(l)) if l.len() == 3)
        })
    });
    thread.update(cx, |t, cx| t.close_picker(cx));

    // Send prompt and confirm reply streams under GEMINI_HOME.
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_keystrokes("h i");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    });
    wait_until(cx, "antigravity reply", |cx| {
        thread.read_with(cx, |t, _| {
            assert_eq!(t.error, None, "thread errored");
            t.items.iter().any(|i| matches!(i, Item::Done { .. }))
        })
    });
    let (row, items, provider) = thread.read_with(cx, |t, _| {
        (
            t.thread.clone().expect("row"),
            t.items.clone(),
            t.provider(),
        )
    });
    assert_eq!(row.account_id, "agy");
    assert_eq!(row.agent_session_id.as_deref(), Some("acp_sess_fake"));
    assert_eq!(provider, Some(Provider::Antigravity));
    assert!(
        items
            .iter()
            .any(|i| matches!(i, Item::Text(t) if t == "Hello from fake Antigravity."))
    );

    // Stored env check.
    let stored: Vec<Record> = {
        let s = store.lock().expect("lock");
        s.events_since(row.id, 0)
            .expect("events")
            .iter()
            .map(|e| Record::from_row(&e.kind, &e.payload_json).expect("parse"))
            .collect()
    };
    let env = stored
        .iter()
        .find_map(|r| match r {
            Record::Agent(skyelab_agent::Event::Unknown(v)) if v["method"] == "fake/env" => {
                Some(v["params"].clone())
            }
            _ => None,
        })
        .expect("fake/env stored");
    assert_eq!(env["gemini_home"], gemini_home.to_str().expect("utf-8"));

    // Handoff availability: an Antigravity thread offers ContinueInClaude, not ContinueInAntigravity.
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    let (claude_ok, agy_ok) = cx.update(|window, cx| {
        (
            window.is_action_available(&ContinueInClaude, cx),
            window.is_action_available(&ContinueInAntigravity, cx),
        )
    });
    assert!(claude_ok && !agy_ok);

    // Rebind project to Claude and hand off.
    let mut rebound = AccountsConfig::load(&accounts_path).expect("load");
    rebound.bindings.clear();
    rebound
        .add_binding(Binding::new(project.clone(), "acme".into()))
        .expect("rebind");
    rebound.save(&accounts_path).expect("save");
    cx.update(|_, cx| cx.set_global(AccountsFile(accounts_path.clone())));
    ws.update(cx, |ws, cx| {
        ws.config = rebound;
        ws.push_config(cx);
    });
    cx.run_until_parked();
    cx.dispatch_action(HandOffThread {
        id: row.id,
        to: Provider::Claude,
    });
    wait_until(cx, "handoff created", |cx| {
        ws.read_with(cx, |ws, _| {
            ws.threads
                .iter()
                .any(|t| t.forked_from == Some(row.id) && t.account_id == "acme")
        })
    });
}
