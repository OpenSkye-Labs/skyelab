//! A thread started with "Run in a new worktree": the worktree, the account it runs under, the
//! Changes tab that follows it, its terminal and its discard. Real git, the fake agent.

use super::changes_model_test::{git, repo};
use super::thread::{Stop, ThreadView, ThreadViewEvent};
use super::thread_model::Record;
use super::thread_worktree::{DiscardWorktree, OpenChanges, ToggleWorktree};
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_core::{Account, Binding};
use std::rc::Rc;
use std::time::Duration;

pub(super) struct Fixture {
    pub _tmp: tempfile::TempDir,
    pub project: PathBuf,
    pub account_dir: PathBuf,
    pub config: AccountsConfig,
    pub store: SharedStore,
}

/// A git repo bound to "acme", plus a second account "other" that nothing is bound to yet.
pub(super) fn fixture() -> Fixture {
    let (tmp, project) = repo(&[("README.md", b"# demo\n")]);
    let root = project.parent().expect("parent").to_path_buf();
    let account_dir = root.join(".claude-acme");
    let mut config = AccountsConfig::default();
    for (id, dir) in [
        ("acme", account_dir.clone()),
        ("other", root.join(".claude-other")),
    ] {
        config
            .add_account(Account {
                id: id.into(),
                label: id.into(),
                config_dir: dir,
            })
            .expect("add account");
    }
    config
        .add_binding(Binding::new(project.clone(), "acme".into()))
        .expect("bind");
    Fixture {
        _tmp: tmp,
        project,
        account_dir,
        config,
        store: Arc::new(Mutex::new(Store::open_in_memory().expect("store"))),
    }
}

/// A workspace on the fixture's project with a new thread tab open and its prompt focused.
pub(super) fn open_thread<'a>(
    f: &Fixture,
    cx: &'a mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<ThreadView>,
    &'a mut VisualTestContext,
) {
    cx.executor().allow_parking(); // the fake agent is a real process
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
    let backend = Backend {
        store: f.store.clone(),
        claude_bin: Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh"),
    };
    let (config, folder) = (f.config.clone(), f.project.clone());
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let ws = Workspace::new(folder, None, config, None, backend, window, cx);
        window.focus(&ws.focus, cx);
        ws
    });
    let dir = f.project.clone();
    ws.update(cx, |ws, cx| ws.add_project(dir, cx));
    wait_until(cx, "project stored", |cx| {
        ws.read_with(cx, |ws, _| ws.project.is_some())
    });
    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    let thread = ws
        .read_with(cx, |ws, _| ws.open_threads.last().and_then(|t| t.upgrade()))
        .expect("thread tab");
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    (ws, thread, cx)
}

pub(super) fn wait_until(
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

pub(super) fn send(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("f i x space i t");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    });
    cx.run_until_parked();
}

/// The fake agent's `{"type":"fake_env",..}` report, once stored.
fn fake_env(store: &SharedStore, thread_id: i64) -> Option<serde_json::Value> {
    let store = store.lock().expect("store lock");
    store
        .events_since(thread_id, 0)
        .expect("events")
        .iter()
        .find_map(|e| match Record::from_row(&e.kind, &e.payload_json) {
            Ok(Record::Agent(skyelab_agent::Event::Unknown(v))) if v["type"] == "fake_env" => {
                Some(v)
            }
            _ => None,
        })
}

#[gpui::test]
fn worktree_thread_runs_in_the_worktree_under_the_projects_account(cx: &mut TestAppContext) {
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    let turns = Rc::new(std::cell::Cell::new(0));
    let counted = turns.clone();
    let _sub = cx.update(|_, cx| {
        cx.subscribe(&thread, move |_, e: &ThreadViewEvent, _| {
            if matches!(e, ThreadViewEvent::TurnDone) {
                counted.set(counted.get() + 1);
            }
        })
    });
    cx.dispatch_action(ToggleWorktree);
    send(cx);
    wait_until(cx, "thread row", |cx| {
        thread.read_with(cx, |t, _| t.thread.is_some() || t.error.is_some())
    });
    let row = thread.read_with(cx, |t, _| {
        assert_eq!(t.error, None);
        t.thread.clone().expect("row")
    });

    let wt = row.worktree_path.clone().expect("worktree recorded");
    let branch = row.worktree_branch.clone().expect("branch recorded");
    assert!(branch.starts_with("skyelab/fix-it-"), "{branch}");
    assert_eq!(
        wt.parent(),
        Some(f.project.join(".skyelab/worktrees").as_path())
    );
    assert_eq!(
        wt.file_name().and_then(|n| n.to_str()),
        branch.strip_prefix("skyelab/")
    );
    assert_eq!(row.cwd, wt, "the thread runs in its worktree");
    assert_eq!(row.account_id, "acme", "the project's account is recorded");
    git(
        &f.project,
        &["rev-parse", "--verify", &format!("refs/heads/{branch}")],
    );
    let exclude = std::fs::read_to_string(f.project.join(".git/info/exclude")).expect("exclude");
    assert!(exclude.lines().any(|l| l == "/.skyelab/"), "{exclude}");
    let stored = f
        .store
        .lock()
        .expect("lock")
        .thread(row.id)
        .expect("read")
        .expect("row");
    assert_eq!(stored.worktree_path.as_deref(), Some(wt.as_path()));
    assert_eq!(stored.worktree_branch.as_deref(), Some(branch.as_str()));

    wait_until(cx, "the fake's env report", |_| {
        fake_env(&f.store, row.id).is_some()
    });
    let env = fake_env(&f.store, row.id).expect("env");
    assert_eq!(env["cwd"], wt.to_str().expect("utf-8"));
    assert_eq!(env["config_dir"], f.account_dir.to_str().expect("utf-8"));
    assert!(turns.get() >= 1, "the Result event emitted TurnDone");

    // Changes follows the thread: an agent turn ending reloads it.
    cx.dispatch_action(OpenChanges);
    let changes = || {
        ws.read_with(&*cx, |ws, _| {
            ws.changes_views.last().and_then(|c| c.upgrade())
        })
    };
    let view = changes().expect("changes tab opened");
    wait_until(cx, "first load", |cx| {
        view.read_with(cx, |c, _| c.changes.is_some())
    });
    view.read_with(cx, |c, _| {
        assert_eq!(c.dir, wt);
        assert!(c.changes.as_ref().expect("loaded").files.is_empty());
    });
    std::fs::write(wt.join("agent.txt"), "written by the agent\n").expect("write");
    thread.update(cx, |_, cx| cx.emit(ThreadViewEvent::TurnDone));
    wait_until(cx, "reload on TurnDone", |cx| {
        view.read_with(cx, |c, _| {
            c.changes.as_ref().is_some_and(|c| c.files.len() == 1)
        })
    });

    // A terminal there runs under the thread's account, and refuses once the folder means another.
    let opts = worktree::terminal_options(&f.config, &wt, "acme").expect("terminal options");
    assert_eq!(opts.cwd, wt);
    assert_eq!(
        opts.env.get("CLAUDE_CONFIG_DIR").map(String::as_str),
        f.account_dir.to_str()
    );
    let mut rebound = f.config.clone();
    rebound
        .add_binding(Binding::new(
            f.project.join(".skyelab/worktrees"),
            "other".into(),
        ))
        .expect("bind worktrees to another account");
    assert!(worktree::terminal_options(&rebound, &wt, "acme").is_err());

    // Discard: refused while the agent runs, then removes the clean worktree and clears the row.
    std::fs::remove_file(wt.join("agent.txt")).expect("clean up");
    // The Changes tab has focus now, so act on the thread itself.
    thread.update_in(cx, |t, window, cx| {
        t.discard_worktree(&DiscardWorktree, window, cx)
    });
    cx.run_until_parked();
    thread.read_with(cx, |t, _| {
        assert!(t.error.is_some(), "discard while running is refused")
    });
    assert!(wt.exists());
    thread.update_in(cx, |t, window, cx| t.stop(&Stop, window, cx));
    wait_until(cx, "agent stopped", |cx| {
        thread.read_with(cx, |t, _| t.session.is_none())
    });
    thread.update_in(cx, |t, window, cx| {
        t.discard_worktree(&DiscardWorktree, window, cx)
    });
    wait_until(cx, "worktree discarded", |cx| {
        thread.read_with(cx, |t, _| {
            t.thread.as_ref().is_some_and(|t| t.worktree_path.is_none())
        })
    });
    assert!(!wt.exists());
    let stored = f
        .store
        .lock()
        .expect("lock")
        .thread(row.id)
        .expect("read")
        .expect("row");
    assert_eq!(stored.worktree_path, None);
    assert_eq!(stored.worktree_branch.as_deref(), Some(branch.as_str()));
}

#[gpui::test]
fn unchecked_thread_runs_in_the_project_without_a_worktree(cx: &mut TestAppContext) {
    let f = fixture();
    let (_ws, thread, cx) = open_thread(&f, cx);
    send(cx);
    wait_until(cx, "thread row", |cx| {
        thread.read_with(cx, |t, _| t.thread.is_some())
    });
    let row = thread.read_with(cx, |t, _| t.thread.clone().expect("row"));
    assert_eq!(row.cwd, f.project);
    assert_eq!(row.worktree_path, None);
    wait_until(cx, "the fake's env report", |_| {
        fake_env(&f.store, row.id).is_some()
    });
    let env = fake_env(&f.store, row.id).expect("env");
    assert_eq!(env["cwd"], f.project.to_str().expect("utf-8"));
    assert!(!f.project.join(".skyelab").exists());
}
