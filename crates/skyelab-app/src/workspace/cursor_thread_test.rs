//! Acceptance: Settings adds a Cursor account (with `CURSOR_CONFIG_DIR` login terminal options and Chat
//! `Cursor model` default), project terminals isolate `CURSOR_CONFIG_DIR`, a Cursor-bound project lists
//! Cursor models, streams replies and records its session id, handoff works to/from Cursor, and Onboarding
//! discovers `~/.cursor` and checks Cursor CLI version.

use super::github::{CursorBin, agent_login_options, login_args};
use super::thread::ThreadView;
use super::thread_extras::handoff::{ContinueInClaude, ContinueInCursor, HandOffThread};
use super::thread_model::{Item, Record};
use super::*;
use crate::settings::{SaveAccount, Settings};
use crate::test_wait::wait_until;
use gpui::TestAppContext;
use crate::ui::TextInput;
use skyelab_core::thread_defaults::ThreadField;
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use skyelab_store::Store;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn fake_cursor() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-cursor.sh")
}

#[gpui::test]
fn settings_adds_cursor_account_login_and_chat_default(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        crate::workspace::thread_extras::prefs::install(None, cx);
    });
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("cfg").join("accounts.toml");
    let cursor_dir = tmp.path().join("cursor-work");
    let (view, cx) = cx.add_window_view(|window, cx| {
        Settings::new(
            Some(path.clone()),
            AccountsConfig::default(),
            None,
            window,
            cx,
        )
    });
    let set = |input: &Entity<TextInput>, text: &str, cx: &mut gpui::VisualTestContext| {
        let text = text.to_string();
        input.update_in(cx, |s, window, cx| s.set_value(text, window, cx));
    };
    let (id, label, dir) = view.read_with(cx, |v, _| {
        (
            v.id_input.clone(),
            v.label_input.clone(),
            v.dir_input.clone(),
        )
    });
    set(&id, "cur", cx);
    set(&label, "Cursor Work", cx);
    set(&dir, cursor_dir.to_str().expect("utf-8"), cx);
    cx.run_until_parked();
    let toggle = cx
        .debug_bounds("provider-pick-Cursor")
        .expect("the Cursor provider card is drawn");
    cx.simulate_click(toggle.center(), gpui::Modifiers::none());
    view.update_in(cx, |v, window, cx| {
        assert_eq!(v.provider, Provider::Cursor);
        v.save_account(&SaveAccount, window, cx);
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("provider-cur").is_some());
    assert!(cx.debug_bounds("cursor-login-cur").is_some());
    let saved = AccountsConfig::load(&path).expect("saved");
    assert_eq!(saved.accounts[0].provider, Provider::Cursor);

    let opts = agent_login_options(
        Path::new("/opt/cursor-agent"),
        login_args(Provider::Cursor),
        &saved.accounts[0],
    )
    .expect("login options");
    assert_eq!(
        opts.command,
        Some(("/opt/cursor-agent".into(), vec!["login".into()]))
    );
    assert_eq!(
        opts.env,
        HashMap::from([(
            "CURSOR_CONFIG_DIR".into(),
            cursor_dir.to_str().expect("utf-8").into()
        )])
    );

    // Settings -> Chat Cursor model default saves cleanly.
    view.update_in(cx, |v, window, cx| {
        v.cursor_model_input
            .update(cx, |i, cx| i.set_value("composer-1", window, cx));
        v.save_cursor_model(cx);
    });
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| {
            crate::workspace::thread_extras::prefs::get(cx)
                .field(ThreadField::CursorModel)
                .clone()
        })
        .as_deref(),
        Some("composer-1")
    );
}

#[gpui::test]
fn cursor_thread_streams_lists_models_isolates_terminals_and_hands_off(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let fake_claude =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let fake_codex =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-codex.sh");
    let fake_gh =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-git/tests/fixtures/fake-gh.sh");
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
        cx.set_global(CursorBin(fake_cursor()));
        cx.set_global(CodexBin(fake_codex));
        cx.set_global(github::GhBin(fake_gh));
    });

    let home = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(home.path()).expect("canonicalize");
    let project = root.join("demo");
    std::fs::create_dir_all(&project).expect("project dir");
    let cursor_dir = root.join(".cursor-work");
    let claude_dir = root.join(".claude-work");
    std::fs::create_dir_all(cursor_dir.join("chats")).expect("cursor marker");
    let accounts_path = root.join("accounts.toml");
    let mut config = AccountsConfig::default();
    config
        .add_account(Account {
            id: "cur".into(),
            label: "Cursor Work".into(),
            provider: Provider::Cursor,
            config_dir: cursor_dir.clone(),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("add cursor");
    config
        .add_account(Account {
            id: "acme".into(),
            label: "Claude Work".into(),
            provider: Provider::Claude,
            config_dir: claude_dir.clone(),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("add claude");
    config
        .add_binding(Binding::new(project.clone(), "cur".into()))
        .expect("bind");
    config.save(&accounts_path).expect("save");

    // Non-Cursor terminal isolates CURSOR_CONFIG_DIR to an empty private dir.
    let other_proj = root.join("other");
    std::fs::create_dir_all(&other_proj).expect("mkdir");
    let unbound_env = terminal_env(&config, &other_proj, &root.join("unbound")).expect("env");
    assert_eq!(
        unbound_env["CURSOR_CONFIG_DIR"],
        root.join("unbound/cursor").to_str().expect("utf-8")
    );

    let store: SharedStore = Arc::new(Mutex::new(Store::open_in_memory().expect("store")));
    let backend = Backend {
        store: store.clone(),
        claude_bin: fake_claude,
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

    // Composer model picker populates from `cursor-agent models`.
    thread.update_in(cx, |t, window, cx| t.toggle_picker(window, cx));
    wait_until(cx, "Cursor models listed", |cx| {
        thread.read_with(cx, |t, cx| {
            t.picker_rows(cx)
                .iter()
                .any(|(id, _)| id.as_deref() == Some("composer-1"))
        })
    });
    thread.update(cx, |t, cx| t.close_picker(cx));

    // Sending a prompt runs fake-cursor.sh and streams reply + tool call.
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_keystrokes("h i");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    });
    wait_until(cx, "cursor reply", |cx| {
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
    assert_eq!(row.account_id, "cur");
    assert_eq!(row.agent_session_id.as_deref(), Some("cur_fake_sess_1"));
    assert_eq!(provider, Some(Provider::Cursor));
    assert!(
        items
            .iter()
            .any(|i| matches!(i, Item::Text(t) if t == "Hello from fake cursor."))
    );
    let stored: Vec<Record> = {
        let store = store.lock().expect("store lock");
        store
            .events_since(row.id, 0)
            .expect("events")
            .iter()
            .map(|e| Record::from_row(&e.kind, &e.payload_json).expect("parses"))
            .collect()
    };
    let env = stored
        .iter()
        .find_map(|r| match r {
            Record::Agent(skyelab_agent::Event::Unknown(v)) if v["type"] == "fake_env" => {
                Some(v.clone())
            }
            _ => None,
        })
        .expect("fake_env stored");
    assert_eq!(
        env["cursor_config_dir"],
        cursor_dir.to_str().expect("utf-8")
    );

    // Handoff from Cursor -> Claude is offered and creates a linked thread once rebound.
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    let (can_claude, can_cursor) = cx.update(|window, cx| {
        (
            window.is_action_available(&ContinueInClaude, cx),
            window.is_action_available(&ContinueInCursor, cx),
        )
    });
    assert!(can_claude && !can_cursor);

    let mut rebound = config.clone();
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
    cx.dispatch_action(HandOffThread {
        id: row.id,
        to: Provider::Claude,
    });
    wait_until(cx, "handoff from cursor to claude", |cx| {
        ws.read_with(cx, |ws, _| {
            ws.threads.iter().any(|t| t.forked_from == Some(row.id))
        })
    });

    // Onboarding lists Cursor CLI and discovers ~/.cursor.
    let discover_home = root.join("discover");
    std::fs::create_dir_all(discover_home.join(".cursor/chats")).expect("mkdir");
    ws.update_in(cx, |ws, window, cx| {
        let setup = ws.welcome.read(cx).setup.clone();
        setup.update(cx, |o, _| o.home = Some(discover_home.clone()));
        ws.open_onboarding(&OpenOnboarding, window, cx);
    });
    let setup = ws.read_with(cx, |ws, cx| ws.welcome.read(cx).setup.clone());
    wait_until(cx, "onboarding detects Cursor", |cx| {
        setup.read_with(cx, |o, _| {
            o.agents.iter().any(|a| {
                a.name == "Cursor" && a.version == Some(Ok("cursor-agent 0.99.0".into()))
            }) && o
                .found
                .iter()
                .any(|f| f.found.provider == Provider::Cursor && f.found.id == "cursor")
        })
    });
}
