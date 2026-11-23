//! Acceptance: a project bound to a Codex account runs its threads with the codex binary under that
//! account's `CODEX_HOME`, and the header names the provider. The codex here is the agent crate's fake.

use super::thread::ThreadView;
use super::thread_model::{Item, Record};
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_core::{Account, Binding, Provider};
use skyelab_store::Store;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn fake_codex() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-codex.sh")
}

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

#[gpui::test]
fn a_codex_bound_project_runs_codex_and_says_so(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
        cx.set_global(CodexBin(fake_codex()));
    });
    let home = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(home.path()).expect("canonicalize");
    let project = root.join("demo");
    std::fs::create_dir_all(&project).expect("project dir");
    let codex_home = root.join(".codex-oai");
    let accounts_path = root.join("accounts.toml");
    let mut config = AccountsConfig::default();
    config
        .add_account(Account {
            id: "oai".into(),
            label: "OpenAI".into(),
            provider: Provider::Codex,
            config_dir: codex_home.clone(),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("add account");
    config
        .add_binding(Binding::new(project.clone(), "oai".into()))
        .expect("bind");
    config.save(&accounts_path).expect("save");
    let store: SharedStore = Arc::new(Mutex::new(Store::open_in_memory().expect("store")));
    // A claude that can't start: picking it instead of codex fails the thread.
    let backend = Backend {
        store: store.clone(),
        claude_bin: root.join("no-such-claude"),
    };
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let ws = Workspace::new(
            root.clone(),
            Some(accounts_path),
            config,
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
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_keystrokes("h i");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    });
    wait_until(cx, "codex's reply", |cx| {
        thread.read_with(cx, |t, _| {
            assert_eq!(t.error, None, "the thread showed an error");
            t.items.iter().any(|i| matches!(i, Item::Done { .. }))
        })
    });
    let (row, items, provider, badge) = thread.read_with(cx, |t, _| {
        (
            t.thread.clone().expect("row"),
            t.items.clone(),
            t.provider(),
            t.badge(),
        )
    });
    assert_eq!(row.account_id, "oai");
    assert_eq!(row.agent_session_id.as_deref(), Some("thr_fake"));
    assert_eq!(provider, Some(Provider::Codex));
    assert_eq!(provider.map(Provider::label), Some("Codex"));
    assert_eq!(badge.as_deref(), Some("OpenAI · oai"));
    assert!(
        items
            .iter()
            .any(|i| matches!(i, Item::Text(t) if t == "Hello from fake codex."))
    );
    assert!(
        items
            .iter()
            .any(|i| matches!(i, Item::Tool(t) if t.result.is_some()))
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
            Record::Agent(skyelab_agent::Event::Unknown(v)) if v["method"] == "fake/env" => {
                Some(v["params"].clone())
            }
            _ => None,
        })
        .expect("the fake's env report is stored");
    assert_eq!(env["codex_home"], codex_home.to_str().expect("utf-8"));
}
