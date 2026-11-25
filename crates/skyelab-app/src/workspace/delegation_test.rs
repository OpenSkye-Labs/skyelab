//! End to end: a fake claude calls SkyeLab's MCP tools through the real proxy and server, and the account rule holds.
#![cfg(unix)]

use super::orchestrate::{self, Orchestration};
use super::thread::{Stop, ThreadView};
use super::thread_model::Record;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::mcp::{Caller, Host as _, Proxy, Tool};
use skyelab_agent::{Block, Event};
use skyelab_core::{Account, Binding};
use skyelab_store::NewThread;

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

pub(super) struct Rig {
    pub(super) _home: tempfile::TempDir,
    pub(super) store: SharedStore,
    pub(super) work_a: i64,
    pub(super) work_b: i64,
    personal: i64,
    unbound: i64,
    /// A thread recorded under the other account.
    other_thread: i64,
}

pub(super) fn rig() -> (Rig, PathBuf, AccountsConfig) {
    rig_with(|_| Store::open_in_memory().expect("store"))
}

/// [`rig`] over the store `open` makes, given the rig's home dir.
pub(super) fn rig_with(open: impl FnOnce(&Path) -> Store) -> (Rig, PathBuf, AccountsConfig) {
    let home = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(home.path()).expect("canonicalize");
    let mut config = AccountsConfig::default();
    for id in ["work", "me"] {
        config
            .add_account(Account {
                id: id.into(),
                label: format!("{id} account"),
                config_dir: root.join(format!(".claude-{id}")),
                provider: Default::default(),
                gh_config_dir: None,
                ssh_agent: false,
            })
            .expect("account");
    }
    let store = open(home.path());
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

pub(super) fn open<'a>(
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
    // The app's path: the queue goes to the hub, which hands it to the next window when this one closes.
    cx.update(|cx| set_hub(None, Some(calls), cx));
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let mut ws = Workspace::new(folder, Some(accounts), config, None, backend, window, cx);
        ws.claim_hub(window, cx);
        window.focus(&ws.focus, cx);
        ws
    });
    cx.run_until_parked();
    wait_until(cx, "projects loaded", |cx| {
        ws.read_with(cx, |ws, _| ws.projects.len() == 4)
    });
    (ws, cx)
}

pub(super) use crate::test_wait::wait_until;

/// A thread tab in `project`, with `prompt` sent from it.
pub(super) fn start(
    ws: &Entity<Workspace>,
    project: i64,
    prompt: &str,
    cx: &mut VisualTestContext,
) -> Entity<ThreadView> {
    cx.dispatch_action(SelectProject { id: project });
    wait_until(cx, "project selected", |cx| {
        ws.read_with(cx, |ws, _| {
            ws.project.as_ref().map(|p| p.id) == Some(project)
        })
    });
    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    let view = ws
        .read_with(cx, |ws, _| ws.open_threads.last().cloned())
        .expect("tab opened");
    send(&view, prompt, cx);
    view
}

pub(super) fn send(view: &Entity<ThreadView>, prompt: &str, cx: &mut VisualTestContext) {
    view.update_in(cx, |v, window, cx| v.submit_remote(prompt, window, cx))
        .expect("prompt taken");
}

/// The `n`th MCP tool result the thread stored: (is_error, text).
pub(super) fn tool_result(store: &SharedStore, thread: i64, n: usize) -> Option<(bool, String)> {
    let rows = store
        .lock()
        .expect("lock")
        .events_since(thread, 0)
        .expect("events");
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

pub(super) fn wait_result(
    cx: &mut VisualTestContext,
    store: &SharedStore,
    thread: i64,
    n: usize,
) -> (bool, String) {
    let mut got = None;
    wait_until(cx, "the MCP tool result", |_| {
        got = tool_result(store, thread, n);
        got.is_some()
    });
    got.expect("waited")
}

pub(super) fn thread_id(view: &Entity<ThreadView>, cx: &mut VisualTestContext) -> i64 {
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
        &format!(
            r#"mcp:start_thread:{{"project_id":{},"prompt":"write the docs"}}"#,
            rig.work_b
        ),
        cx,
    );
    let parent_id = thread_id(&parent, cx);
    let (err, text) = wait_result(cx, &store, parent_id, 0);
    assert!(!err, "same-account start refused: {text}");
    let child = store
        .lock()
        .expect("lock")
        .list_threads(rig.work_b)
        .expect("threads");
    assert_eq!(child.len(), 1, "one child in work-b");
    assert_eq!(child[0].account_id, "work");
    assert_eq!(child[0].parent_thread_id, Some(parent_id));
    assert_eq!(child[0].title, "write the docs");
    assert!(text.contains(&child[0].id.to_string()), "{text}");
    // Nothing set, so the user's own thread is in Full access; the agent-started child still runs in Ask.
    let parent_row = store.lock().expect("lock").thread(parent_id).expect("read");
    assert_eq!(parent_row.expect("row").permission_mode, "bypass");
    assert_eq!(child[0].permission_mode, "default");

    // The parent can read and wait on its child.
    send(
        &parent,
        &format!(
            r#"mcp:wait_thread:{{"thread_id":{},"timeout_s":20}}"#,
            child[0].id
        ),
        cx,
    );
    let (err, text) = wait_result(cx, &store, parent_id, 1);
    assert!(!err && text.contains("done"), "{text}");
    send(
        &parent,
        &format!(r#"mcp:read_thread:{{"thread_id":{}}}"#, child[0].id),
        cx,
    );
    let (err, text) = wait_result(cx, &store, parent_id, 2);
    assert!(
        !err && text.contains("Done with the delegated task"),
        "{text}"
    );

    // Another account's project: refused, nothing created.
    send(
        &parent,
        &format!(
            r#"mcp:start_thread:{{"project_id":{},"prompt":"spend it"}}"#,
            rig.personal
        ),
        cx,
    );
    let (err, text) = wait_result(cx, &store, parent_id, 3);
    assert!(err, "cross-account start allowed: {text}");
    assert!(text.contains("work") && text.contains("me"), "{text}");

    // An unbound project: refused, nothing created.
    send(
        &parent,
        &format!(
            r#"mcp:start_thread:{{"project_id":{},"prompt":"anything"}}"#,
            rig.unbound
        ),
        cx,
    );
    let (err, text) = wait_result(cx, &store, parent_id, 4);
    assert!(err && text.contains("not bound"), "{text}");

    // Another account's thread can't be read or driven.
    for (n, call) in [
        format!(r#"mcp:read_thread:{{"thread_id":{}}}"#, rig.other_thread),
        format!(
            r#"mcp:send_message:{{"thread_id":{},"text":"hi"}}"#,
            rig.other_thread
        ),
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
    assert!(
        !text.contains("personal") && !text.contains("loose"),
        "{text}"
    );

    let guard = store.lock().expect("lock");
    assert!(guard.list_threads(rig.unbound).expect("threads").is_empty());
    assert_eq!(
        guard.list_threads(rig.personal).expect("threads").len(),
        1,
        "only the pre-made one"
    );
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
        assert!(
            v.children.iter().any(|c| c.id == child[0].id),
            "parent lists its child"
        );
    });

    // A stricter project default wins: a child in a Plan project starts in Plan.
    let dir = store
        .lock()
        .expect("lock")
        .project(rig.work_b)
        .expect("read")
        .expect("project")
        .path;
    cx.update(|_, cx| {
        crate::workspace::thread_extras::prefs::edit(
            move |t| {
                t.set_project_field(
                    &dir,
                    skyelab_core::thread_defaults::ThreadField::PermissionMode,
                    Some("plan".into()),
                )
            },
            cx,
        )
    })
    .detach();
    send(
        &parent,
        &format!(
            r#"mcp:start_thread:{{"project_id":{},"prompt":"plan it"}}"#,
            rig.work_b
        ),
        cx,
    );
    let (err, text) = wait_result(cx, &store, parent_id, 9);
    assert!(!err, "{text}");
    let planned = store
        .lock()
        .expect("lock")
        .list_threads(rig.work_b)
        .expect("threads")
        .into_iter()
        .find(|t| t.title == "plan it")
        .expect("the second child");
    assert_eq!(planned.permission_mode, "plan");

    // The grant ends with the parent's agent.
    let before = cx.update(|_, cx| cx.global::<Orchestration>().server.active_grants());
    parent.update_in(cx, |v, window, cx| v.stop(&Stop, window, cx));
    wait_until(cx, "the parent's grant dropped", |cx| {
        cx.update(|_, cx| cx.global::<Orchestration>().server.active_grants()) < before
    });
}

/// Runs a host call off the UI thread (it waits for the UI's answer) while the test keeps the UI running.
pub(super) fn host_call(
    cx: &mut VisualTestContext,
    caller: Caller,
    tool: Tool,
) -> Result<serde_json::Value, String> {
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
        request_id: None,
    };

    // Four live children are allowed, the fifth is refused. `hold` keeps each one mid-turn.
    for _ in 0..4 {
        host_call(cx, caller(), start_tool("hold")).expect("under the limit");
    }
    let err = host_call(cx, caller(), start_tool("child 5")).expect_err("fifth child");
    assert!(err.contains("children"), "{err}");
    assert_eq!(
        store
            .lock()
            .expect("lock")
            .list_threads(rig.work_b)
            .expect("list")
            .len(),
        4
    );

    // A grandchild (depth 2) can't delegate further.
    let kids = store
        .lock()
        .expect("lock")
        .child_threads(parent_id)
        .expect("kids");
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
    assert_eq!(
        store
            .lock()
            .expect("lock")
            .child_threads(grandchild)
            .expect("kids")
            .len(),
        0
    );

    // A caller claiming an account its thread wasn't recorded under is refused.
    let liar = Caller {
        thread_id: parent_id,
        account_id: "me".into(),
    };
    assert!(host_call(cx, liar, Tool::ListProjects).is_err());
}

pub(super) fn statuses(store: &SharedStore, parent: i64) -> Vec<String> {
    let s = store.lock().expect("lock");
    s.child_threads(parent)
        .expect("children")
        .into_iter()
        .map(|t| t.status)
        .collect()
}

#[gpui::test]
fn finished_children_free_their_slot_and_only_descendants_can_be_driven(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let store = rig.store.clone();
    let parent = start(&ws, rig.work_a, "plain work", cx);
    let parent_id = thread_id(&parent, cx);
    let other = start(&ws, rig.work_a, "the user's own work", cx);
    let other_id = thread_id(&other, cx);
    let caller = || Caller {
        thread_id: parent_id,
        account_id: "work".into(),
    };
    let start_tool = |p: &str| Tool::StartThread {
        project_id: rig.work_b,
        prompt: p.into(),
        worktree: false,
        request_id: None,
    };

    // Children that finished their turn don't count against the limit.
    for n in 0..4 {
        host_call(cx, caller(), start_tool(&format!("child {n}"))).expect("under the limit");
    }
    wait_until(cx, "four idle children", |_| {
        let s = statuses(&store, parent_id);
        s.len() == 4 && s.iter().all(|s| s == "idle")
    });
    host_call(cx, caller(), start_tool("child 5")).expect("finished children don't count");

    // A same-account thread the caller didn't start is out of reach.
    for tool in [
        Tool::ThreadStatus {
            thread_id: other_id,
        },
        Tool::ReadThread {
            thread_id: other_id,
            since_seq: 0,
        },
        Tool::SendMessage {
            thread_id: other_id,
            text: "hi".into(),
        },
        Tool::WaitThread {
            thread_id: other_id,
            timeout_s: 1,
        },
    ] {
        let what = format!("{tool:?}");
        host_call(cx, caller(), tool).expect_err(&what);
    }
    let kid = store
        .lock()
        .expect("lock")
        .child_threads(parent_id)
        .expect("kids")[0]
        .id;
    let child_caller = Caller {
        thread_id: kid,
        account_id: "work".into(),
    };
    let to_parent = Tool::SendMessage {
        thread_id: parent_id,
        text: "loop".into(),
    };
    host_call(cx, child_caller, to_parent).expect_err("a child can't drive its parent");
    let read = Tool::ReadThread {
        thread_id: kid,
        since_seq: 0,
    };
    host_call(cx, caller(), read).expect("a parent reads its own child");

    // A child the user stopped isn't restarted by a message.
    let kid_view = ws
        .read_with(cx, |ws, cx| ws.open_view(kid, cx))
        .expect("child tab");
    kid_view.update_in(cx, |v, window, cx| v.stop(&Stop, window, cx));
    wait_until(cx, "child stopped", |cx| {
        let row = store
            .lock()
            .expect("lock")
            .thread(kid)
            .expect("row")
            .expect("kid");
        row.status == "stopped" && kid_view.read_with(cx, |v, _| v.session.is_none())
    });
    let again = Tool::SendMessage {
        thread_id: kid,
        text: "again".into(),
    };
    host_call(cx, caller(), again).expect_err("a stopped child stays stopped");
    cx.run_until_parked();
    let status = store
        .lock()
        .expect("lock")
        .thread(kid)
        .expect("row")
        .expect("kid")
        .status;
    assert_eq!(status, "stopped");
}
