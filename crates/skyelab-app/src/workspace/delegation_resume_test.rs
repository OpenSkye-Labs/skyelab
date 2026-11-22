//! A resumed session gets SkyeLab's MCP tools too, not only a new one.
#![cfg(unix)]

use super::orchestrate;
use super::thread::{Stop, ThreadView};
use super::thread_model::Record;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::mcp::Proxy;
use skyelab_agent::{Block, Event};
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

/// How many SkyeLab MCP tool results the thread stored.
fn mcp_results(store: &SharedStore, thread: i64) -> usize {
    store
        .lock()
        .expect("lock")
        .events_since(thread, 0)
        .expect("events")
        .iter()
        .filter_map(|e| Record::from_row(&e.kind, &e.payload_json).ok())
        .filter(|r| match r {
            Record::Agent(Event::User { content }) => content.iter().any(|b| {
                matches!(b, Block::ToolResult { tool_use_id, .. } if tool_use_id.starts_with("toolu_mcp"))
            }),
            _ => false,
        })
        .count()
}

fn send(view: &Entity<ThreadView>, prompt: &str, cx: &mut VisualTestContext) {
    view.update_in(cx, |v, window, cx| v.submit_remote(prompt, window, cx))
        .expect("prompt taken");
}

#[gpui::test]
fn a_resumed_session_still_gets_the_orchestration_tools(cx: &mut TestAppContext) {
    let home = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(home.path()).expect("canonicalize");
    let mut config = AccountsConfig::default();
    config
        .add_account(Account {
            id: "work".into(),
            label: "work account".into(),
            config_dir: root.join(".claude-work"),
            provider: Default::default(),
            gh_config_dir: None,
        })
        .expect("account");
    let dir = root.join("work-a");
    std::fs::create_dir_all(&dir).expect("mkdir");
    config
        .add_binding(Binding::new(dir.clone(), "work".into()))
        .expect("bind");
    let store = Store::open_in_memory().expect("store");
    let project = store.create_project("work-a", &dir).expect("project").id;
    let store: SharedStore = Arc::new(Mutex::new(store));
    let accounts = root.join("accounts.toml");
    config.save(&accounts).expect("save");

    cx.executor().allow_parking();
    let proxy = Proxy {
        program: std::env::current_exe().expect("test binary"),
        args: vec![
            "workspace::delegation_test::mcp_proxy_child".into(),
            "--exact".into(),
            "--nocapture".into(),
        ],
    };
    let (run_dir, s, cfg) = (root.join("run"), store.clone(), config.clone());
    let calls = cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
        orchestrate::install(cx, &run_dir, s, cfg, proxy).expect("orchestration")
    });
    let backend = Backend {
        store: store.clone(),
        claude_bin: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/fake-claude-delegate.sh"),
    };
    let folder = root.clone();
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let mut ws = Workspace::new(folder, Some(accounts), config, None, backend, window, cx);
        ws.set_orchestrator(calls, window, cx);
        window.focus(&ws.focus, cx);
        ws
    });
    wait_until(cx, "projects loaded", |cx| {
        ws.read_with(cx, |ws, _| ws.projects.len() == 1)
    });
    cx.dispatch_action(SelectProject { id: project });
    wait_until(cx, "project selected", |cx| {
        ws.read_with(cx, |ws, _| {
            ws.project.as_ref().map(|p| p.id) == Some(project)
        })
    });
    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    let view = ws
        .read_with(cx, |ws, _| ws.open_threads.last().and_then(|t| t.upgrade()))
        .expect("tab opened");

    send(&view, "mcp:list_projects:{}", cx);
    let mut id = None;
    wait_until(cx, "the thread row with a session id", |cx| {
        id = view.read_with(cx, |v, _| {
            v.thread
                .as_ref()
                .filter(|t| t.agent_session_id.is_some())
                .map(|t| t.id)
        });
        id.is_some()
    });
    let id = id.expect("waited");
    wait_until(cx, "the first session's MCP tool result", |_| {
        mcp_results(&store, id) == 1
    });

    // Stop, then resume the same thread: the new process must get a grant too.
    view.update_in(cx, |v, window, cx| v.stop(&Stop, window, cx));
    wait_until(cx, "the agent stopped and is resumable", |cx| {
        view.read_with(cx, |v, _| {
            v.session.is_none() && !v.read_only && !v.starting
        })
    });
    send(&view, "mcp:list_projects:{}", cx);
    wait_until(cx, "the resumed session's MCP tool result", |_| {
        mcp_results(&store, id) == 2
    });
}
