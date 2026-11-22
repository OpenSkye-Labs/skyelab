//! End to end: a fake claude calls SkyeLab's MCP tools through the real proxy and server, and the account rule holds.
#![cfg(unix)]

use super::thread::{Stop, ThreadView};
use super::thread_model::Record;
use super::*;
use crate::orchestrate::{self, Orchestration};
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::mcp::{Caller, Host as _, Proxy, Tool};
use skyelab_agent::{Block, Event};
use skyelab_core::{Account, Binding};
use skyelab_store::NewThread;
use std::time::Duration;

/// The proxy claude spawns. Here it is this test binary re-run on [`mcp_proxy_child`].
#[test]
fn mcp_proxy_child() {
    if std::env::var_os(skyelab_agent::mcp::SOCKET_ENV).is_none() {
        return; // a normal test run
    }
    let code = match skyelab_agent::mcp::proxy_main() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("proxy: {e:#}");
            1
        }
    };
    std::process::exit(code);
}

struct Rig {
    _home: tempfile::TempDir,
    store: SharedStore,
    work_a: i64,
    work_b: i64,
    personal: i64,
    unbound: i64,
    /// A thread recorded under the other account.
    other_thread: i64,
}

fn rig() -> (Rig, PathBuf, AccountsConfig) {
    let home = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(home.path()).expect("canonicalize");
    let mut config = AccountsConfig::default();
    for id in ["work", "me"] {
        config
            .add_account(Account {
                id: id.into(),
                label: format!("{id} account"),
                config_dir: root.join(format!(".claude-{id}")),
            })
            .expect("account");
    }
    let store = Store::open_in_memory().expect("store");
    let mut ids = Vec::new();
    for (name, account) in [
        ("work-a", Some("work")),
        ("work-b", Some("work")),
        ("personal", Some("me")),
        ("loose", None),
    ] {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).expect("mkdir");
        if let Some(a) = account {
            config
                .add_binding(Binding::new(dir.clone(), a.into()))
                .expect("bind");
        }
        ids.push(store.create_project(name, &dir).expect("project").id);
    }
    let other_thread = store
        .create_thread(&NewThread {
            project_id: ids[2],
            title: "private",
            cwd: &root.join("personal"),
            account_id: "me",
            status: "idle",
        })
        .expect("thread")
        .id;
    let accounts = root.join("accounts.toml");
    config.save(&accounts).expect("save");
    let rig = Rig {
        _home: home,
        store: Arc::new(Mutex::new(store)),
        work_a: ids[0],
        work_b: ids[1],
        personal: ids[2],
        unbound: ids[3],
        other_thread,
    };
    (rig, accounts, config)
}

fn open<'a>(
    rig: &Rig,
    accounts: PathBuf,
    config: AccountsConfig,
    cx: &'a mut TestAppContext,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    let run_dir = rig._home.path().join("run");
    let store = rig.store.clone();
    let proxy = Proxy {
        program: std::env::current_exe().expect("test binary"),
        args: vec![
            "workspace::delegation_test::mcp_proxy_child".into(),
            "--exact".into(),
            "--nocapture".into(),
        ],
    };
    let cfg = config.clone();
    let calls = cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
        orchestrate::install(cx, &run_dir, store, cfg, proxy).expect("orchestration")
    });
    let backend = Backend {
        store: rig.store.clone(),
        claude_bin: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/fake-claude-delegate.sh"),
    };
    let folder = rig._home.path().to_path_buf();
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let mut ws = Workspace::new(folder, Some(accounts), config, None, backend, window, cx);
        ws.set_orchestrator(calls, window, cx);
        ws
    });
    cx.run_until_parked();
    (ws, cx)
}

fn wait_until(cx: &mut VisualTestContext, what: &str, mut done: impl FnMut(&mut VisualTestContext) -> bool) {
    for _ in 0..1500 {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

/// A thread tab in `project`, with `prompt` sent from it.
fn start(ws: &Entity<Workspace>, project: i64, prompt: &str, cx: &mut VisualTestContext) -> Entity<ThreadView> {
    cx.dispatch_action(SelectProject { id: project });
    wait_until(cx, "project selected", |cx| {
        ws.read_with(cx, |ws, _| ws.project.as_ref().map(|p| p.id) == Some(project))
    });
    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    let view = ws
        .read_with(cx, |ws, _| ws.open_threads.last().and_then(|t| t.upgrade()))
        .expect("tab opened");
    send(&view, prompt, cx);
    view
}

fn send(view: &Entity<ThreadView>, prompt: &str, cx: &mut VisualTestContext) {
    view.update_in(cx, |v, window, cx| v.submit_remote(prompt, window, cx))
        .expect("prompt taken");
}

/// The `n`th MCP tool result the thread stored: (is_error, text).
fn tool_result(store: &SharedStore, thread: i64, n: usize) -> Option<(bool, String)> {
    let rows = store.lock().expect("lock").events_since(thread, 0).expect("events");
    rows.iter()
        .filter_map(|e| Record::from_row(&e.kind, &e.payload_json).ok())
        .filter_map(|r| match r {
            Record::Agent(Event::User { content }) => content.into_iter().find_map(|b| match b {
                Block::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } if tool_use_id.starts_with("toolu_mcp") => {
                    Some((is_error, content.as_str().unwrap_or_default().to_owned()))
                }
                _ => None,
            }),
            _ => None,
        })
        .nth(n)
}

fn wait_result(cx: &mut VisualTestContext, store: &SharedStore, thread: i64, n: usize) -> (bool, String) {
    let mut got = None;
    wait_until(cx, "the MCP tool result", |_| {
        got = tool_result(store, thread, n);
        got.is_some()
    });
    got.expect("waited")
}

fn thread_id(view: &Entity<ThreadView>, cx: &mut VisualTestContext) -> i64 {
    let mut id = None;
    wait_until(cx, "the thread row", |cx| {
        id = view.read_with(cx, |v, _| v.thread.as_ref().map(|t| t.id));
        id.is_some()
    });
    id.expect("waited")
}

#[gpui::test]
fn delegation_runs_under_the_target_account_and_refuses_other_accounts(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let store = rig.store.clone();

    // Same account: a child thread in work-b, under work, linked to its parent.
    let parent = start(
        &ws,
        rig.work_a,
        &format!(r#"mcp:start_thread:{{"project_id":{},"prompt":"write the docs"}}"#, rig.work_b),
        cx,
    );
    let parent_id = thread_id(&parent, cx);
    let (err, text) = wait_result(cx, &store, parent_id, 0);
    assert!(!err, "same-account start refused: {text}");
    let child = store.lock().expect("lock").list_threads(rig.work_b).expect("threads");
    assert_eq!(child.len(), 1, "one child in work-b");
    assert_eq!(child[0].account_id, "work");
    assert_eq!(child[0].parent_thread_id, Some(parent_id));
    assert_eq!(child[0].title, "write the docs");
    assert!(text.contains(&child[0].id.to_string()), "{text}");

    // The parent can read and wait on its child.
    send(&parent, &format!(r#"mcp:wait_thread:{{"thread_id":{},"timeout_s":20}}"#, child[0].id), cx);
    let (err, text) = wait_result(cx, &store, parent_id, 1);
    assert!(!err && text.contains("done"), "{text}");
    send(&parent, &format!(r#"mcp:read_thread:{{"thread_id":{}}}"#, child[0].id), cx);
    let (err, text) = wait_result(cx, &store, parent_id, 2);
    assert!(!err && text.contains("Done with the delegated task"), "{text}");

    // Another account's project: refused, nothing created.
    send(&parent, &format!(r#"mcp:start_thread:{{"project_id":{},"prompt":"spend it"}}"#, rig.personal), cx);
    let (err, text) = wait_result(cx, &store, parent_id, 3);
    assert!(err, "cross-account start allowed: {text}");
    assert!(text.contains("work") && text.contains("me"), "{text}");

    // An unbound project: refused, nothing created.
    send(&parent, &format!(r#"mcp:start_thread:{{"project_id":{},"prompt":"anything"}}"#, rig.unbound), cx);
    let (err, text) = wait_result(cx, &store, parent_id, 4);
    assert!(err && text.contains("not bound"), "{text}");

    // Another account's thread can't be read or driven.
    for (n, call) in [
        format!(r#"mcp:read_thread:{{"thread_id":{}}}"#, rig.other_thread),
        format!(r#"mcp:send_message:{{"thread_id":{},"text":"hi"}}"#, rig.other_thread),
        format!(r#"mcp:thread_status:{{"thread_id":{}}}"#, rig.other_thread),
    ]
    .iter()
    .enumerate()
    {
        send(&parent, call, cx);
        let (err, text) = wait_result(cx, &store, parent_id, 5 + n);
        assert!(err, "{call} allowed: {text}");
    }

    // Projects listed are only the caller's account's.
    send(&parent, "mcp:list_projects:{}", cx);
    let (err, text) = wait_result(cx, &store, parent_id, 8);
    assert!(!err, "{text}");
    assert!(text.contains("work-a") && text.contains("work-b"), "{text}");
    assert!(!text.contains("personal") && !text.contains("loose"), "{text}");

    let guard = store.lock().expect("lock");
    assert!(guard.list_threads(rig.unbound).expect("threads").is_empty());
    assert_eq!(guard.list_threads(rig.personal).expect("threads").len(), 1, "only the pre-made one");
    let events = guard.events_since(rig.other_thread, 0).expect("events");
    assert!(events.is_empty(), "the other account's thread got nothing");
    drop(guard);

    // The child's UI links back to its parent, and the parent lists its child.
    let child_view = ws
        .read_with(cx, |ws, cx| ws.open_view(child[0].id, cx))
        .expect("child tab open");
    child_view.read_with(cx, |v, _| {
        assert_eq!(v.parent.as_ref().map(|p| p.0), Some(parent_id));
    });
    parent.read_with(cx, |v, _| {
        assert!(v.children.iter().any(|c| c.id == child[0].id), "parent lists its child");
    });

    // The grant ends with the parent's agent.
    let grants = || cx.update(|_, cx| cx.global::<Orchestration>().server.active_grants());
    let before = grants();
    parent.update_in(cx, |v, window, cx| v.stop(&Stop, window, cx));
    wait_until(cx, "the parent's grant dropped", |cx| {
        cx.update(|_, cx| cx.global::<Orchestration>().server.active_grants()) < before
    });
}

/// Runs a host call off the UI thread (it waits for the UI's answer) while the test keeps the UI running.
fn host_call(cx: &mut VisualTestContext, caller: Caller, tool: Tool) -> Result<serde_json::Value, String> {
    let host = cx.update(|_, cx| cx.global::<Orchestration>().host.clone());
    let call = std::thread::spawn(move || host.call(&caller, tool));
    wait_until(cx, "the host call", |_| call.is_finished());
    call.join().expect("host call panicked")
}

#[gpui::test]
fn depth_and_children_limits_hold_through_the_app(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let store = rig.store.clone();
    let parent = start(&ws, rig.work_a, "plain work", cx);
    let parent_id = thread_id(&parent, cx);
    let caller = || Caller {
        thread_id: parent_id,
        account_id: "work".into(),
    };
    let start_tool = |p: &str| Tool::StartThread {
        project_id: rig.work_b,
        prompt: p.into(),
        worktree: false,
    };

    // Four live children are allowed, the fifth is refused.
    for n in 0..4 {
        host_call(cx, caller(), start_tool(&format!("child {n}"))).expect("under the limit");
    }
    let err = host_call(cx, caller(), start_tool("child 5")).expect_err("fifth child");
    assert!(err.contains("children"), "{err}");
    assert_eq!(store.lock().expect("lock").list_threads(rig.work_b).expect("list").len(), 4);

    // A grandchild (depth 2) can't delegate further.
    let kids = store.lock().expect("lock").child_threads(parent_id).expect("kids");
    let child = kids[0].id;
    let grandchild = {
        let s = store.lock().expect("lock");
        let t = s
            .create_thread(&NewThread {
                project_id: rig.work_b,
                title: "grandchild",
                cwd: &kids[0].cwd,
                account_id: "work",
                status: "running",
            })
            .expect("row");
        s.set_thread_parent(t.id, Some(child)).expect("link");
        t.id
    };
    let deep = Caller {
        thread_id: grandchild,
        account_id: "work".into(),
    };
    let err = host_call(cx, deep, start_tool("too deep")).expect_err("depth 3");
    assert!(err.contains("depth"), "{err}");
    assert_eq!(store.lock().expect("lock").child_threads(grandchild).expect("kids").len(), 0);

    // A caller claiming an account its thread wasn't recorded under is refused.
    let liar = Caller {
        thread_id: parent_id,
        account_id: "me".into(),
    };
    assert!(host_call(cx, liar, Tool::ListProjects).is_err());
}
