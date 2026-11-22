use super::thread::{Interrupt, Stop, ThreadView};
use super::thread_model::{Item, Record};
use super::*;
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use skyelab_core::{Account, Binding};
use skyelab_store::{Store, Thread};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn fake_bin() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh")
}

fn setup(cx: &mut TestAppContext) {
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

struct Fixture {
    _home: tempfile::TempDir,
    project_dir: PathBuf,
    account_dir: PathBuf,
    accounts_path: PathBuf,
    store: SharedStore,
}

/// A project folder, optionally bound to "acme", and an in-memory store.
fn fixture(bound: bool) -> Fixture {
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

fn open_workspace<'a>(
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

/// Runs the app until `done` holds, giving the fake agent's threads real time to write.
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

/// Waits for the fake's result, failing with the view's own error if the start or a write failed.
fn wait_done(thread: &Entity<ThreadView>, cx: &mut VisualTestContext) {
    wait_until(cx, "the fake's result", |cx| {
        thread.read_with(cx, |t, _| {
            assert_eq!(t.error, None, "the thread showed an error");
            t.items.iter().any(|i| matches!(i, Item::Done { .. }))
        })
    });
}

/// Adds the fixture's folder through the workspace, selects it and opens a new thread tab.
fn project_thread(
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
    ws.read_with(cx, |ws, _| ws.open_threads.last().and_then(|t| t.upgrade()))
        .expect("new thread tab opened")
}

/// Types into the thread's prompt like a user and sends with cmd-enter (ctrl-enter off macOS).
fn type_and_send(thread: &Entity<ThreadView>, cx: &mut VisualTestContext) {
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_keystrokes("h i enter t h e r e");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    });
    cx.run_until_parked();
}

fn stored_records(store: &SharedStore, thread_id: i64) -> Vec<Record> {
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
    let opts = ws.read_with(cx, |ws, _| ws.terminal_options().expect("options"));
    assert_eq!(opts.cwd, f.project_dir, "terminals open in the project");
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
    let env = terminal_env(&config, &f.project_dir).expect("env");
    assert_eq!(
        env.get("CLAUDE_CONFIG_DIR").map(String::as_str),
        f.account_dir.to_str()
    );
    // Unbound: no CLAUDE_CONFIG_DIR, so skyelab-terminal clears the inherited one.
    let parent = f.project_dir.parent().expect("parent");
    assert!(terminal_env(&config, parent).expect("env").is_empty());
}

#[test]
fn claude_bin_override_is_debug_only() {
    let fake = Some(std::ffi::OsString::from("/tmp/fake-claude"));
    assert_eq!(
        crate::claude_bin(fake.clone(), true),
        Path::new("/tmp/fake-claude")
    );
    assert_eq!(crate::claude_bin(fake, false), Path::new("claude"));
    assert_eq!(crate::claude_bin(None, true), Path::new("claude"));
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
        assert!(
            ws.settings.as_ref().and_then(|s| s.upgrade()).is_some(),
            "the CTA opens Settings"
        )
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
    reopened.read_with(cx, |t, _| {
        assert_eq!(t.items, live, "replay rebuilds the same items");
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

/// Runs a thread to its result, stops it, and returns its store row.
fn stopped_thread<'a>(
    f: &Fixture,
    cx: &'a mut TestAppContext,
) -> (Thread, Entity<Workspace>, &'a mut VisualTestContext) {
    let (ws, cx) = open_workspace(f, cx);
    let thread = project_thread(f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    thread.update_in(cx, |t, window, cx| t.stop(&Stop, window, cx));
    wait_until(cx, "the session to stop", |cx| {
        thread.read_with(cx, |t, _| t.session.is_none())
    });
    let row = f
        .store
        .lock()
        .expect("lock")
        .thread(thread.read_with(cx, |t, _| t.thread.as_ref().expect("row").id))
        .expect("read")
        .expect("row");
    assert_eq!(row.agent_session_id.as_deref(), Some("fake-sess-1"));
    (row, ws, cx)
}

/// Reopens `row` like the sidebar does, with the accounts file as it is now.
fn reopen(f: &Fixture, row: &Thread, cx: &mut VisualTestContext) -> Entity<ThreadView> {
    let config = AccountsConfig::load(&f.accounts_path).expect("load");
    let backend = Backend {
        store: f.store.clone(),
        claude_bin: fake_bin(),
    };
    let row = row.clone();
    let view =
        cx.new_window_entity(|window, cx| ThreadView::open(row, config, backend, window, cx));
    wait_until(cx, "the row to load", |cx| {
        view.read_with(cx, |t, _| !t.read_only)
    });
    view
}

/// Sends `text` from a view that isn't the window's root, so keystrokes can't reach it.
fn send_text(view: &Entity<ThreadView>, text: &'static str, cx: &mut VisualTestContext) {
    view.update_in(cx, |t, window, cx| {
        t.prompt.update(cx, |p, cx| p.set_value(text, window, cx));
        t.send_prompt(&super::thread::SendPrompt, window, cx);
    });
    cx.run_until_parked();
}

fn env_reports(store: &SharedStore, thread_id: i64) -> Vec<serde_json::Value> {
    stored_records(store, thread_id)
        .into_iter()
        .filter_map(|r| match r {
            Record::Agent(skyelab_agent::Event::Unknown(v)) if v["type"] == "fake_env" => Some(v),
            _ => None,
        })
        .collect()
}

/// Rebinds the fixture's folder: to `to`, or unbound when `None`.
fn rebind(f: &Fixture, to: Option<&str>) {
    let mut config = AccountsConfig::load(&f.accounts_path).expect("load");
    config
        .add_account(Account {
            id: "other".into(),
            label: "Other Co".into(),
            config_dir: f.account_dir.with_file_name(".claude-other"),
        })
        .expect("add account");
    config.remove_binding(&f.project_dir).expect("unbind");
    if let Some(to) = to {
        config
            .add_binding(Binding::new(f.project_dir.clone(), to.into()))
            .expect("bind");
    }
    config.save(&f.accounts_path).expect("save");
}

#[cfg(unix)] // the fake agent is a shell script
#[gpui::test]
fn reopened_thread_resumes_under_its_account(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (row, _ws, cx) = stopped_thread(&f, cx);
    let view = reopen(&f, &row, cx);
    send_text(&view, "again", cx);
    wait_until(cx, "the resumed agent's env report", |_| {
        env_reports(&f.store, row.id).len() == 2
    });
    let resumed = &env_reports(&f.store, row.id)[1];
    let args = resumed["args"].as_str().expect("args");
    assert!(args.contains("--resume fake-sess-1"), "{args}");
    assert_eq!(
        resumed["config_dir"],
        f.account_dir.to_str().expect("utf-8")
    );
    view.read_with(cx, |t, _| {
        assert!(t.session.is_some(), "the resumed session runs");
        assert_eq!(t.resume_refused, None);
        assert_eq!(t.thread.as_ref().map(|t| t.id), Some(row.id), "same row");
    });
    let rows = f.store.lock().expect("lock").list_threads(row.project_id);
    assert_eq!(rows.expect("list").len(), 1, "resuming adds no thread row");
}

fn assert_refused(f: &Fixture, row: &Thread, cx: &mut VisualTestContext, says: &str) {
    let config = AccountsConfig::load(&f.accounts_path).expect("load");
    let backend = Backend {
        store: f.store.clone(),
        claude_bin: fake_bin(),
    };
    let row = row.clone();
    // Its own window, with the view as root, so the refusal is drawn and its button can be found.
    let (view, cx) = std::ops::DerefMut::deref_mut(cx)
        .add_window_view(|window, cx| ThreadView::open(row.clone(), config, backend, window, cx));
    wait_until(cx, "the row to load", |cx| {
        view.read_with(cx, |t, _| !t.read_only)
    });
    send_text(&view, "again", cx);
    view.read_with(cx, |t, _| {
        let why = t.resume_refused.clone().expect("resume refused");
        assert!(why.contains(says), "{why}");
        assert!(t.session.is_none(), "nothing started");
        assert!(!t.starting);
    });
    std::thread::sleep(Duration::from_millis(300));
    cx.run_until_parked();
    let thread_id = view.read_with(cx, |t, _| t.thread.as_ref().map(|t| t.id));
    assert_eq!(
        env_reports(&f.store, thread_id.expect("row")).len(),
        1,
        "no second agent ran"
    );
    assert!(
        cx.debug_bounds("new-thread-cta").is_some(),
        "offers a new thread"
    );
}

#[cfg(unix)] // the fake agent is a shell script
#[gpui::test]
fn resume_refuses_another_account(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (row, _ws, cx) = stopped_thread(&f, cx);
    rebind(&f, Some("other"));
    assert_refused(&f, &row, cx, "now bound to Other Co");
}

#[cfg(unix)] // the fake agent is a shell script
#[gpui::test]
fn resume_refuses_unbound_folder(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (row, _ws, cx) = stopped_thread(&f, cx);
    rebind(&f, None);
    assert_refused(&f, &row, cx, "isn't bound to acme");
}
