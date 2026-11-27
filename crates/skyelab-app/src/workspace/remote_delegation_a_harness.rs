//! The calling machine (A) for the delegation tests: a workspace with orchestration on, one live parent thread in a
//! project bound to `work` (signed in with an email and org id), and machine B linked: the shared harness's
//! in-process daemon, whose recording backend answers `delegate` for A's (node, key, account `work`).

use super::env_harness::{RemoteSide, RemoteSideExt as _};
use super::env_linked::ToggleLinkedEnvironment;
use super::orchestrate::{self, Orchestration};
use super::thread::ThreadView;
use super::*;
use crate::test_wait::wait_until;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::mcp::{Caller, Host as _, Proxy, Tool};
use skyelab_core::{Account, AppSettings, Binding};
use skyelab_daemon::testing::{DEVICE_KEY, FakeTailnet, NODE_NAME};
use skyelab_protocol::device_key_hash;

pub(super) const B: &str = NODE_NAME;

pub(super) struct A {
    pub _home: tempfile::TempDir,
    pub db: PathBuf,
    pub store: SharedStore,
    pub ws: Entity<Workspace>,
    pub b: RemoteSide,
    pub parent: i64,
    pub project: skyelab_store::Project,
}

/// A, with B linked and (when `on`) "Let my agents delegate here" on for it.
pub(super) fn a_side(on: bool, cx: &mut TestAppContext) -> (A, &mut VisualTestContext) {
    cx.executor().allow_parking();
    let mut b = RemoteSide::with(FakeTailnet::default(), "/remote/app", "fix it");
    b.agents
        .delegation_links
        .lock()
        .expect("links")
        .push(("nPhone".into(), device_key_hash(DEVICE_KEY), "work".into()));
    b.serve();
    let home = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(home.path()).expect("canonicalize");
    let config_dir = root.join(".cfg-work");
    std::fs::create_dir_all(&config_dir).expect("mkdir");
    std::fs::write(config_dir.join("fake-login"), "me@acme.com").expect("login");
    std::fs::write(config_dir.join("fake-org"), "org-acme").expect("org");
    let dir = root.join("app");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let mut config = AccountsConfig::default();
    config
        .add_account(Account {
            id: "work".into(),
            label: "Work".into(),
            provider: Default::default(),
            config_dir,
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("account");
    config
        .add_binding(Binding::new(dir.clone(), "work".into()))
        .expect("bind");
    let accounts = root.join("accounts.toml");
    config.save(&accounts).expect("accounts");
    let settings = root.join("settings.toml");
    AppSettings::default().save(&settings).expect("settings");
    let db = root.join("store.db");
    let store: SharedStore = Arc::new(Mutex::new(Store::open(&db).expect("store")));
    let project = lock(&store)
        .and_then(|s| s.create_project("app", &dir))
        .expect("project");
    let (run, store2, cfg) = (root.join("run"), store.clone(), config.clone());
    let calls = cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
        let proxy = Proxy {
            program: "/usr/bin/false".into(),
            args: vec![],
        };
        orchestrate::install(cx, &run, store2, cfg, proxy).expect("orchestration")
    });
    cx.update(|cx| set_hub(None, Some(calls), cx));
    let backend = Backend {
        store: store.clone(),
        claude_bin: Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh"),
    };
    let (options, folder, settings2) = (b.options(), root.clone(), settings.clone());
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let mut ws = Workspace::new(folder, Some(accounts), config, None, backend, window, cx);
        ws.claim_hub(window, cx);
        ws.set_environments(Arc::new(FakeTailnet::default()), Some(settings2), options, window, cx);
        ws
    });
    ws.update_in(cx, |_, window, cx| {
        window.dispatch_action(Box::new(ToggleLinkedEnvironment { host: B.into() }), cx)
    });
    wait_until(cx, "B linked and connected", |cx| {
        ws.read_with(cx, |ws, cx| ws.linked_envs().iter().any(|e| e.read(cx).node().is_some()))
    });
    if on {
        AppSettings::update(&settings, |s| s.set_delegation_target(B, true)).expect("switch on");
    }
    // A live parent: its agent (the fake) keeps running between turns.
    let (cfg, backend) = ws.read_with(cx, |ws, _| (ws.config.clone(), ws.backend.clone()));
    let p = project.clone();
    let view = ws.update_in(cx, |ws, window, cx| {
        let view = cx.new(|cx| ThreadView::new(&p, cfg, backend, window, cx));
        ws.add_thread_tab(view.clone(), false, window, cx);
        view
    });
    view.update_in(cx, |v, window, cx| v.submit_remote("lead the work", window, cx))
        .expect("the parent starts");
    wait_until(cx, "the parent's agent", |cx| {
        view.read_with(cx, |v, _| v.session.is_some() && v.thread.is_some())
    });
    let parent = view.read_with(cx, |v, _| v.thread.as_ref().map(|t| t.id)).expect("row");
    let a = A {
        _home: home,
        db,
        store,
        ws,
        b,
        parent,
        project,
    };
    (a, cx)
}

/// The parent's tool call `tool` sent to B, off the UI thread while the test keeps the UI running.
pub(super) fn on_b(a: &A, tool: Tool, cx: &mut VisualTestContext) -> Result<serde_json::Value, String> {
    let host = cx.update(|_, cx| cx.global::<Orchestration>().host.clone());
    let caller = Caller {
        thread_id: a.parent,
        account_id: "work".into(),
    };
    let tool = Tool::Remote {
        machine: B.into(),
        tool: Box::new(tool),
    };
    let call = std::thread::spawn(move || host.call(&caller, tool));
    wait_until(cx, "the host call", |_| call.is_finished());
    call.join().expect("host call panicked")
}

pub(super) fn machines(a: &A, cx: &mut VisualTestContext) -> serde_json::Value {
    let host = cx.update(|_, cx| cx.global::<Orchestration>().host.clone());
    let caller = Caller {
        thread_id: a.parent,
        account_id: "work".into(),
    };
    let call = std::thread::spawn(move || host.call(&caller, Tool::ListMachines));
    wait_until(cx, "list_machines", |_| call.is_finished());
    call.join().expect("panicked").expect("listed")
}

pub(super) fn delegate_calls(a: &A) -> Vec<String> {
    a.b.ran()
        .into_iter()
        .filter_map(|r| match r {
            skyelab_daemon::testing::Ran::Delegate(_, op) => Some(op),
            _ => None,
        })
        .collect()
}

pub(super) fn start(project_id: i64) -> Tool {
    Tool::StartThread {
        project_id,
        prompt: "do it there".into(),
        worktree: false,
        request_id: None,
    }
}

/// Rows under the parent that are working on a turn.
pub(super) fn busy_local_children(a: &A, n: usize) {
    let s = lock(&a.store).expect("lock");
    for _ in 0..n {
        let t = s
            .create_thread(&skyelab_store::NewThread {
                project_id: a.project.id,
                title: "local child",
                cwd: &a.project.path,
                account_id: "work",
                status: "running",
            })
            .expect("thread");
        s.set_thread_parent(t.id, Some(a.parent)).expect("parent");
    }
}

/// A child the parent started on B earlier.
pub(super) fn known_child(a: &A, thread_id: i64) {
    lock(&a.store)
        .and_then(|s| {
            s.add_remote_child(&skyelab_store::RemoteChild {
                parent_thread_id: a.parent,
                machine: B.into(),
                node_id: "nSelf".into(),
                thread_id,
                title: "remote child".into(),
                status: String::new(),
                created_at: 0,
            })
        })
        .expect("child");
}
