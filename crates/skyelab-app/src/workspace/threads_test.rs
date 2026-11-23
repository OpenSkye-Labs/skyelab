use super::thread::ThreadView;
#[cfg(unix)]
use super::thread::{Interrupt, Stop};
#[cfg(unix)]
use super::thread_model::{Item, Record};
use super::*;
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use skyelab_core::{Account, Binding};
use skyelab_store::Store;
#[cfg(unix)]
use skyelab_store::Thread;
use std::sync::{Arc, Mutex};

pub(super) fn fake_bin() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh")
}

pub(super) fn setup(cx: &mut TestAppContext) {
    // The fake agent and the terminals are real processes whose reader threads wake the UI.
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
}

/// A backend for tests that never start an agent: an in-memory store, and the fake as the binary.
pub(super) fn test_backend() -> Backend {
    Backend {
        store: Arc::new(Mutex::new(Store::open_in_memory().expect("store"))),
        claude_bin: fake_bin(),
    }
}

pub(super) struct Fixture {
    pub(super) _home: tempfile::TempDir,
    pub(super) project_dir: PathBuf,
    pub(super) account_dir: PathBuf,
    pub(super) accounts_path: PathBuf,
    pub(super) store: SharedStore,
}

/// A project folder, optionally bound to "acme", and an in-memory store.
pub(super) fn fixture(bound: bool) -> Fixture {
    let home = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(home.path()).expect("canonicalize");
    let project_dir = root.join("demo");
    std::fs::create_dir_all(&project_dir).expect("project dir");
    let account_dir = root.join(".claude-acme");
    let accounts_path = root.join("accounts.toml");
    let mut config = AccountsConfig::default();
    config
        .add_account(Account {
            id: "acme".into(),
            label: "Acme Team".into(),
            config_dir: account_dir.clone(),
            provider: Default::default(),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("add account");
    if bound {
        config
            .add_binding(Binding::new(project_dir.clone(), "acme".into()))
            .expect("bind");
    }
    config.save(&accounts_path).expect("save");
    Fixture {
        _home: home,
        project_dir,
        account_dir,
        accounts_path,
        store: Arc::new(Mutex::new(Store::open_in_memory().expect("store"))),
    }
}

pub(super) fn open_workspace<'a>(
    f: &Fixture,
    cx: &'a mut TestAppContext,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    let config = AccountsConfig::load(&f.accounts_path).expect("load");
    let backend = Backend {
        store: f.store.clone(),
        claude_bin: fake_bin(),
    };
    let folder = f.project_dir.parent().expect("parent").to_path_buf();
    let path = f.accounts_path.clone();
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let ws = Workspace::new(folder, Some(path), config, None, backend, window, cx);
        window.focus(&ws.focus, cx);
        ws
    });
    cx.run_until_parked();
    (ws, cx)
}

pub(super) use crate::test_wait::wait_until;

/// Waits for the fake's result, failing with the view's own error if the start or a write failed.
#[cfg(unix)]
pub(super) fn wait_done(thread: &Entity<ThreadView>, cx: &mut VisualTestContext) {
    wait_until(cx, "the fake's result", |cx| {
        thread.read_with(cx, |t, _| {
            assert_eq!(t.error, None, "the thread showed an error");
            t.items.iter().any(|i| matches!(i, Item::Done { .. }))
        })
    });
}

/// Adds the fixture's folder through the workspace, selects it and opens a new thread tab.
pub(super) fn project_thread(
    f: &Fixture,
    ws: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Entity<ThreadView> {
    let dir = f.project_dir.clone();
    ws.update(cx, |ws, cx| ws.add_project(dir, cx));
    wait_until(cx, "project stored", |cx| {
        ws.read_with(cx, |ws, _| !ws.projects.is_empty())
    });
    let id = ws.read_with(cx, |ws, _| ws.projects[0].id);
    cx.dispatch_action(SelectProject { id });
    cx.run_until_parked();
    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| ws.open_threads.last().cloned())
        .expect("new thread tab opened")
}

/// Types into the thread's prompt like a user and sends with cmd-enter (ctrl-enter off macOS).
pub(super) fn type_and_send(thread: &Entity<ThreadView>, cx: &mut VisualTestContext) {
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_keystrokes("h i enter t h e r e");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    });
    cx.run_until_parked();
}

#[cfg(unix)]
pub(super) fn stored_records(store: &SharedStore, thread_id: i64) -> Vec<Record> {
    let store = store.lock().expect("store lock");
    store
        .events_since(thread_id, 0)
        .expect("events")
        .iter()
        .map(|e| Record::from_row(&e.kind, &e.payload_json).expect("stored record parses"))
        .collect()
}

#[gpui::test]
fn add_project_lists_it_and_selecting_sets_folder_and_account(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    ws.read_with(cx, |ws, _| assert_eq!(ws.account, None, "home is unbound"));
    let _ = project_thread(&f, &ws, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.projects.len(), 1);
        assert_eq!(ws.folder, f.project_dir);
        assert_eq!(ws.account.as_deref(), Some("Acme Team"));
    });
    let job = ws.read_with(cx, |ws, cx| ws.terminal_job(cx));
    let (opts, account) = job().expect("options");
    assert_eq!(opts.cwd, f.project_dir, "terminals open in the project");
    assert_eq!(
        account.map(|a| a.label).as_deref(),
        Some("Acme Team"),
        "the tab shows the account the env was built for"
    );
    assert_eq!(
        opts.env.get("CLAUDE_CONFIG_DIR").map(String::as_str),
        f.account_dir.to_str(),
        "terminals run under the project's account"
    );
    let listed = f.store.lock().expect("lock").list_projects().expect("list");
    assert_eq!(listed.len(), 1, "the project is in the store");
    assert_eq!(listed[0].path, f.project_dir);

    // Adding the same folder again selects the existing project instead of failing.
    let dir = f.project_dir.clone();
    ws.update(cx, |ws, cx| ws.add_project(dir, cx));
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| assert_eq!(ws.projects.len(), 1));
}

#[test]
fn terminal_env_is_the_bound_account_or_empty() {
    let f = fixture(true);
    let config = AccountsConfig::load(&f.accounts_path).expect("load");
    let unbound = tempfile::tempdir().expect("tempdir");
    let env = terminal_env(&config, &f.project_dir, unbound.path()).expect("env");
    assert_eq!(
        env.get("CLAUDE_CONFIG_DIR").map(String::as_str),
        f.account_dir.to_str()
    );
    // Unbound: an empty "unbound" home, never the account's or the default one.
    let parent = f.project_dir.parent().expect("parent");
    let env = terminal_env(&config, parent, unbound.path()).expect("env");
    assert_eq!(
        env.get("CLAUDE_CONFIG_DIR").map(String::as_str),
        unbound.path().join("claude").to_str()
    );
}

#[test]
fn store_lives_in_the_platform_data_dir() {
    let expected = dirs::data_dir().map(|d| d.join("skyelab").join("skyelab.db"));
    assert_eq!(crate::store_path(), expected);
}

#[gpui::test]
fn unbound_project_shows_cta_and_starts_nothing(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(false);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    type_and_send(&thread, cx);
    thread.read_with(cx, |t, _| {
        assert!(t.needs_binding, "the bind CTA shows");
        assert!(t.session.is_none(), "no session started");
        assert!(t.thread.is_none(), "no thread row");
    });
    let project = ws.read_with(cx, |ws, _| ws.projects[0].id);
    let rows = f
        .store
        .lock()
        .expect("lock")
        .list_threads(project)
        .expect("list");
    assert!(rows.is_empty(), "nothing stored for an unbound folder");

    let cta = cx.debug_bounds("bind-cta").expect("CTA button rendered");
    cx.simulate_click(cta.center(), Modifiers::none());
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert!(ws.settings.clone().is_some(), "the CTA opens Settings")
    });
}

#[cfg(unix)] // the fake agent is a shell script
#[gpui::test]
fn bound_project_streams_and_persists(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);

    let (row, items, badge) = thread.read_with(cx, |t, _| {
        (
            t.thread.clone().expect("thread row"),
            t.items.clone(),
            t.badge(),
        )
    });
    assert_eq!(row.account_id, "acme", "the thread row records its account");
    assert_eq!(row.cwd, f.project_dir);
    assert_eq!(badge.as_deref(), Some("Acme Team · acme"));
    assert_eq!(
        items[0],
        Item::Prompt("hi\nthere".into()),
        "enter adds a newline"
    );
    assert!(
        items
            .iter()
            .any(|i| matches!(i, Item::Text(t) if t.contains("four crates")))
    );
    assert!(
        items
            .iter()
            .any(|i| matches!(i, Item::Tool(t) if t.result.is_some()))
    );
    assert!(items.iter().any(|i| matches!(
        i,
        Item::Done {
            duration_ms: 4210,
            ..
        }
    )));

    wait_until(cx, "the fake's env report persisted", |_| {
        stored_records(&f.store, row.id).iter().any(|r| {
            matches!(r, Record::Agent(skyelab_agent::Event::Unknown(v)) if v["type"] == "fake_env")
        })
    });
    let records = stored_records(&f.store, row.id);
    assert_eq!(records[0], Record::Prompt("hi\nthere".into()));
    let env = records
        .iter()
        .find_map(|r| match r {
            Record::Agent(skyelab_agent::Event::Unknown(v)) if v["type"] == "fake_env" => Some(v),
            _ => None,
        })
        .expect("env report");
    assert_eq!(env["config_dir"], f.account_dir.to_str().expect("utf-8"));
    assert_eq!(env["cwd"], f.project_dir.to_str().expect("utf-8"));
    assert!(
        records
            .iter()
            .any(|r| matches!(r, Record::Agent(skyelab_agent::Event::Result { .. }))),
        "every agent event is stored"
    );

    // Tool rows start collapsed and toggle open.
    let tool_id = items
        .iter()
        .find_map(|i| match i {
            Item::Tool(t) => Some(t.id.clone()),
            _ => None,
        })
        .expect("tool row");
    thread.read_with(cx, |t, _| assert!(!t.is_expanded(&tool_id)));
    thread.update(cx, |t, cx| t.toggle_tool(&tool_id, cx));
    thread.read_with(cx, |t, _| assert!(t.is_expanded(&tool_id)));
}

#[cfg(unix)] // the fake agent is a shell script
#[gpui::test]
fn reopen_replays_from_store(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    let (row, live) =
        thread.read_with(cx, |t, _| (t.thread.clone().expect("row"), t.items.clone()));

    // A row this build can't read (say, an older format) is skipped, not fatal to the replay.
    f.store
        .lock()
        .expect("lock")
        .append_event(row.id, "from-the-future", "{}")
        .expect("append");
    let config = AccountsConfig::load(&f.accounts_path).expect("load");
    let backend = Backend {
        store: f.store.clone(),
        claude_bin: fake_bin(),
    };
    let reopened = cx
        .new_window_entity(|window, cx| ThreadView::open(row.clone(), config, backend, window, cx));
    wait_until(cx, "replay", |cx| {
        reopened.read_with(cx, |t, _| t.items.len() >= live.len())
    });
    // A turn's end time is when the view saw it live, and when the store saved it on replay (saves are async).
    let undated = |items: &[Item]| -> Vec<Item> {
        items
            .iter()
            .cloned()
            .map(|mut i| {
                if let Item::Done { at_ms, .. } = &mut i {
                    assert!(at_ms.is_some(), "every turn end is dated");
                    *at_ms = None;
                }
                i
            })
            .collect()
    };
    reopened.read_with(cx, |t, _| {
        assert_eq!(
            undated(&t.items),
            undated(&live),
            "replay rebuilds the same items"
        );
        assert!(t.session.is_none(), "replay starts no agent");
        assert_eq!(t.error.as_deref(), Some("1 stored events couldn't be read"));
        assert_eq!(t.badge().as_deref(), Some("Acme Team · acme"));
    });
}

#[cfg(unix)] // the fake agent is a shell script
#[gpui::test]
fn interrupt_is_sent_and_stop_kills(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    let row = thread.read_with(cx, |t, _| t.thread.clone().expect("row"));

    thread.update_in(cx, |t, window, cx| t.interrupt(&Interrupt, window, cx));
    // The fake echoes stdin, so the interrupt request comes back as an event.
    wait_until(cx, "the echoed interrupt", |_| {
        stored_records(&f.store, row.id).iter().any(|r| {
            matches!(r, Record::Agent(skyelab_agent::Event::Unknown(v))
                if v["type"] == "control_request" && v["request"]["subtype"] == "interrupt")
        })
    });

    let session = thread
        .read_with(cx, |t, _| t.session.clone())
        .expect("session running");
    thread.update_in(cx, |t, window, cx| t.stop(&Stop, window, cx));
    wait_until(cx, "the session to stop", |cx| {
        thread.read_with(cx, |t, _| t.session.is_none())
    });
    let exited = session
        .lock()
        .expect("session lock")
        .try_wait()
        .expect("try_wait");
    assert!(exited.is_some(), "stop killed the agent");
    let status = f
        .store
        .lock()
        .expect("lock")
        .thread(row.id)
        .expect("read")
        .expect("row")
        .status;
    assert_eq!(status, "stopped");
}

#[cfg(unix)] // the fake agent is a shell script
#[path = "threads_resume_test.rs"]
pub(super) mod resume;

#[cfg(unix)] // the fake agent is a shell script
#[path = "session_gone_test.rs"]
mod session_gone;
