//! The serving machine (B) for the delegation tests: a real workspace with sharing on, a real daemon on loopback
//! behind the fake tailnet, the fake claude, and a real client calling `delegate` as the user's other machine (A)
//! would. Accounts: `work` and `personal` (Claude, same email, other orgs) and `cx` (Codex). Projects: `work-a`
//! and `work-b` bound to work, `personal`, `codex`, and `loose` (unbound).

use super::*;
use crate::remote::{DaemonSlot, Host, Remote, Status, UiAgents, push_events};
use crate::test_wait::wait_until;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_core::delegation_remote::{DelegationLink, Login};
use skyelab_core::{Account, AppSettings, ApprovedDevice, Binding, Provider};
use skyelab_daemon::testing::{DEVICE_KEY, FakeTailnet, NODE_NAME, ca};
use skyelab_protocol::{DelegateOp, Delegator, RpcError, device_key_hash};
use std::collections::HashMap;

pub(super) const EMAIL: &str = "me@acme.com";
pub(super) const ORG: &str = "org-acme";
/// The node and name whois gives the test client.
pub(super) const A_NODE: &str = "nPhone";
pub(super) const A_NAME: &str = "phone.example.ts.net";

#[derive(Default)]
pub(super) struct Setup {
    /// The local account the caller (A's `x`) is linked to, if any.
    pub link_to: Option<&'static str>,
    /// B's `work` account has no org id (an API-key login).
    pub no_org: bool,
    /// The store is a file, so a test can reopen it.
    pub disk_store: bool,
    /// `work-a` is a git repo with one commit.
    pub repo: bool,
}

pub(super) struct B {
    pub _dir: tempfile::TempDir,
    pub root: PathBuf,
    pub settings: PathBuf,
    pub db: Option<PathBuf>,
    pub store: SharedStore,
    pub ws: Entity<Workspace>,
    pub remote: Entity<Remote>,
    pub tailnet: Arc<FakeTailnet>,
    pub client: skyelab_client::Client,
    pub projects: HashMap<String, i64>,
}

pub(super) fn link(local: &str) -> DelegationLink {
    DelegationLink {
        node_id: A_NODE.into(),
        key_hash: device_key_hash(DEVICE_KEY),
        machine: A_NAME.into(),
        origin_account: "x".into(),
        login: Login {
            provider: Provider::Claude,
            email: EMAIL.into(),
            org_id: ORG.into(),
        },
        local_account: local.into(),
    }
}

/// A's thread 7 under its account `x`, at depth 0.
pub(super) fn caller() -> Delegator {
    Delegator {
        store_id: "sA".into(),
        thread_id: 7,
        title: "parent".into(),
        account_id: "x".into(),
        login: skyelab_protocol::Login {
            provider: "claude".into(),
            email: EMAIL.into(),
            org_id: ORG.into(),
        },
        depth: 0,
    }
}

pub(super) fn start_op(project_id: i64, request_id: &str) -> DelegateOp {
    DelegateOp::Start {
        project_id,
        prompt: "hello from A".into(),
        worktree: false,
        request_id: request_id.into(),
    }
}

fn git(dir: &Path, args: &[&str]) {
    let mut cmd = std::process::Command::new("git");
    cmd.args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir);
    let out = skyelab_core::process::output(&mut cmd, None).expect("git");
    assert!(out.status.success(), "git {args:?}");
}

pub(super) fn b_side(setup: Setup, cx: &mut TestAppContext) -> (B, &mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
    let dir = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(dir.path()).expect("canonicalize");
    let mut config = AccountsConfig::default();
    for (id, provider, org) in [
        ("work", Provider::Claude, (!setup.no_org).then_some(ORG)),
        ("personal", Provider::Claude, Some("org-personal")),
        ("cx", Provider::Codex, None),
    ] {
        let config_dir = root.join(format!(".cfg-{id}"));
        std::fs::create_dir_all(&config_dir).expect("mkdir");
        std::fs::write(config_dir.join("fake-login"), EMAIL).expect("sign in");
        if let Some(org) = org {
            std::fs::write(config_dir.join("fake-org"), org).expect("org");
        }
        config
            .add_account(Account {
                id: id.into(),
                label: format!("{id} label"),
                provider,
                config_dir,
                gh_config_dir: None,
                ssh_agent: false,
            })
            .expect("account");
    }
    let names = ["work-a", "work-b", "personal", "codex", "loose"];
    for (name, account) in names.iter().zip([Some("work"), Some("work"), Some("personal"), Some("cx"), None]) {
        let d = root.join(name);
        std::fs::create_dir_all(&d).expect("mkdir");
        if *name == "work-a" && setup.repo {
            git(&d, &["init", "-q", "-b", "main"]);
            std::fs::write(d.join("README.md"), "# demo\n").expect("write");
            git(&d, &["add", "."]);
            git(&d, &["commit", "-q", "-m", "init"]);
        }
        if let Some(a) = account {
            config.add_binding(Binding::new(d, a.into())).expect("bind");
        }
    }
    let accounts = root.join("accounts.toml");
    config.save(&accounts).expect("save accounts");
    let settings = root.join("settings.toml");
    let mut s = AppSettings {
        approved_devices: vec![ApprovedDevice {
            node_id: A_NODE.into(),
            key_hash: device_key_hash(DEVICE_KEY),
            name: A_NAME.into(),
            user: "me@example.com".into(),
        }],
        ..AppSettings::default()
    };
    if let Some(local) = setup.link_to {
        s.link_delegation(link(local));
    }
    s.save(&settings).expect("settings");

    let slot = DaemonSlot::default();
    let db = setup.disk_store.then(|| root.join("store.db"));
    let mut store = match &db {
        Some(db) => Store::open(db).expect("store"),
        None => Store::open_in_memory().expect("store"),
    };
    push_events(&mut store, slot.clone());
    let store: SharedStore = Arc::new(Mutex::new(store));
    let tailnet = Arc::new(FakeTailnet::default());
    let (agents, calls) = UiAgents::new();
    let host = Host {
        settings_path: Some(settings.clone()),
        tailnet: tailnet.clone(),
        config: skyelab_daemon::Config {
            port: 0,
            ..skyelab_daemon::Config::default()
        },
        backend: Arc::new(skyelab_daemon::StoreBackend::shared(store.clone(), agents, None)),
        slot,
    };
    let remote = cx.new(|cx| Remote::new(host, cx));
    let backend = Backend {
        store: store.clone(),
        claude_bin: Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh"),
    };
    let (remote2, folder) = (remote.clone(), root.clone());
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let mut ws = Workspace::new(folder, Some(accounts), config, None, backend, window, cx);
        ws.set_remote(remote2, Some(calls), window, cx);
        ws
    });
    for name in names {
        let d = root.join(name);
        ws.update(cx, |ws, cx| ws.add_project(d, cx));
    }
    wait_until(cx, "the projects to load", |cx| {
        ws.read_with(cx, |ws, _| ws.projects.len() == names.len())
    });
    let projects = ws.read_with(cx, |ws, _| {
        ws.projects.iter().map(|p| (p.name.clone(), p.id)).collect()
    });
    remote.update(cx, |r, cx| r.toggle(cx));
    wait_until(cx, "sharing", |cx| {
        remote.read_with(cx, |r, _| matches!(r.status, Status::Sharing { .. }))
    });
    let port = remote.read_with(cx, |r, _| match &r.status {
        Status::Sharing { port, .. } => *port,
        _ => 0,
    });
    let client = skyelab_client::Client::connect(
        NODE_NAME,
        skyelab_client::testing::options(port, ca().root()),
    )
    .expect("client");
    wait_until(cx, "the client's hello", |_| client.hello().is_some());
    let b = B {
        _dir: dir,
        root,
        settings,
        db,
        store,
        ws,
        remote,
        tailnet,
        client,
        projects,
    };
    (b, cx)
}

impl B {
    pub(super) fn project(&self, name: &str) -> i64 {
        self.projects[name]
    }

    pub(super) fn thread_count(&self) -> usize {
        lock(&self.store).and_then(|s| s.list_all_threads()).expect("threads").len()
    }

    pub(super) fn account_dir(&self, id: &str) -> PathBuf {
        self.root.join(format!(".cfg-{id}"))
    }
}

/// One `delegate` call from `client` as `caller`, the UI pumped until it is answered.
pub(super) fn call_with(
    client: &skyelab_client::Client,
    caller: &Delegator,
    op: DelegateOp,
    cx: &mut VisualTestContext,
) -> Result<serde_json::Value, RpcError> {
    let pending = client.delegate(caller.clone(), op);
    let (tx, rx) = flume::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send(pending.recv_timeout(std::time::Duration::from_secs(60)));
    });
    let mut got = None;
    wait_until(cx, "the delegate reply", |_| {
        if got.is_none() {
            got = rx.try_recv().ok();
        }
        got.is_some()
    });
    match got.expect("waited") {
        Ok(v) => Ok(v),
        Err(skyelab_client::Error::Rpc(e)) => Err(e),
        Err(other) => panic!("delegate failed: {other}"),
    }
}

pub(super) fn call(b: &B, caller: &Delegator, op: DelegateOp, cx: &mut VisualTestContext) -> Result<serde_json::Value, RpcError> {
    call_with(&b.client, caller, op, cx)
}

/// A start that must pass: the new thread's id.
pub(super) fn started(b: &B, caller: &Delegator, op: DelegateOp, cx: &mut VisualTestContext) -> i64 {
    let v = call(b, caller, op, cx).expect("the start passes");
    v["thread_id"].as_i64().expect("a thread id")
}

/// Waits until thread `id` has its agent's first reply stored (the fake reports its env first).
pub(super) fn wait_for_agent(b: &B, id: i64, cx: &mut VisualTestContext) -> String {
    let mut seen = String::new();
    wait_until(cx, "the agent's env report", |_| {
        let rows = lock(&b.store).and_then(|s| s.events_since(id, 0)).unwrap_or_default();
        match rows.iter().find(|e| e.payload_json.contains("fake_env")) {
            Some(e) => {
                seen = e.payload_json.clone();
                true
            }
            None => false,
        }
    });
    seen
}

/// Rows with `status` delegated from `node` by `(store, parent)`, as if started earlier.
pub(super) fn seed_children(b: &B, node: &str, store_id: &str, parent: i64, n: usize, status: &str) -> Vec<i64> {
    let s = lock(&b.store).expect("lock");
    let project = s.project(b.project("work-b")).expect("read").expect("project");
    (0..n)
        .map(|_| {
            let t = s
                .create_thread(&skyelab_store::NewThread {
                    project_id: project.id,
                    title: "seeded",
                    cwd: &project.path,
                    account_id: "work",
                    status,
                })
                .expect("thread");
            s.set_delegated_from(
                t.id,
                &skyelab_store::DelegatedOrigin {
                    origin_node: node.into(),
                    origin_store: store_id.into(),
                    origin_machine: A_NAME.into(),
                    parent_thread_id: parent,
                    parent_title: "parent".into(),
                    origin_account: "x".into(),
                    depth: 1,
                    request_id: None,
                },
            )
            .expect("origin");
            t.id
        })
        .collect()
}
