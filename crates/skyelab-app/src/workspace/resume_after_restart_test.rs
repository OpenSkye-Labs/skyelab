//! Acceptance 8: threads left `starting`/`running` by a crash, kill or forced restart auto-resume once with
//! `--resume` when `resume_after_restart` is enabled, skip unresolved approval/question/plan waits, cap continuation
//! chains to 1, respect the setting being off, and reset to on via Restore defaults.

use super::resume_after_restart::{self, RESTART_RESUME_PROMPT};
use super::thread_model::Record;
use super::threads_test::resume::{env_reports, stopped_thread};
use super::threads_test::{Fixture, fixture, open_workspace, setup, stored_records, wait_until};
use crate::settings::{Section, Settings};
use gpui::TestAppContext;
use skyelab_agent::{Event, PLAN_TOOL, QUESTION_TOOL};
use skyelab_core::{AccountsConfig, AppSettings};
use skyelab_store::{NewThread, Store};
use std::path::Path;
use std::sync::{Arc, Mutex};

fn on_disk(f: &mut Fixture) -> std::path::PathBuf {
    let path = f._home.path().join("store.db");
    f.store = Arc::new(Mutex::new(Store::open(&path).expect("store file")));
    path
}

fn reopen_store(f: &mut Fixture, path: &Path) {
    f.store = Arc::new(Mutex::new(Store::open(path).expect("reopen store")));
}

fn restart_prompts(f: &Fixture, id: i64) -> usize {
    stored_records(&f.store, id)
        .iter()
        .filter(|r| matches!(r, Record::Prompt(p) if p.contains("SkyeLab restarted and resumed this turn")))
        .count()
}

fn append_record(f: &Fixture, id: i64, record: &Record) {
    let (kind, payload) = record.to_row();
    f.store
        .lock()
        .expect("lock")
        .append_event(id, kind, &payload)
        .expect("append");
}

#[gpui::test]
fn interrupted_mid_turn_resumes_once_with_session_id(cx: &mut TestAppContext) {
    setup(cx);
    let mut f = fixture(true);
    let db = on_disk(&mut f);
    let settings = f._home.path().join("settings.toml");
    cx.update(|cx| resume_after_restart::install(Some(settings.clone()), cx));

    let (row, _ws, _vcx) = stopped_thread(&f, cx);
    // Simulate a crash mid-turn: a new prompt was sent and the row stayed `running` (and another stayed `starting`).
    append_record(&f, row.id, &Record::Prompt("finish the refactor".into()));
    f.store
        .lock()
        .expect("lock")
        .set_thread_status(row.id, "running")
        .expect("mark running");

    let starting_id = {
        let store = f.store.lock().expect("lock");
        let t = store
            .create_thread(&NewThread {
                project_id: row.project_id,
                title: "starting thread",
                cwd: &f.project_dir,
                account_id: "acme",
                status: "starting",
            })
            .expect("create");
        store
            .set_agent_session_id(t.id, Some("fake-sess-2"))
            .expect("session");
        t.id
    };
    append_record(&f, starting_id, &Record::Prompt("start working".into()));

    // Relaunch: Store::open recovers both to `interrupted` and Workspace auto-resumes them.
    reopen_store(&mut f, &db);
    let (ws, vcx) = open_workspace(&f, cx);
    wait_until(vcx, "both recovered threads resumed", |_| {
        env_reports(&f.store, row.id).len() == 2 && env_reports(&f.store, starting_id).len() == 1
    });
    let resumed = &env_reports(&f.store, row.id)[1];
    let args = resumed["args"].as_str().expect("args");
    assert!(args.contains("--resume fake-sess-1"), "expected --resume: {args}");
    assert_eq!(
        resumed["config_dir"],
        f.account_dir.to_str().expect("utf-8"),
        "resumes under the folder's bound account"
    );
    let resumed_starting = &env_reports(&f.store, starting_id)[0];
    assert!(
        resumed_starting["args"]
            .as_str()
            .expect("args")
            .contains("--resume fake-sess-2")
    );
    assert_eq!(restart_prompts(&f, row.id), 1);
    assert_eq!(restart_prompts(&f, starting_id), 1);

    // Timeline shows the continuation prompt clearly.
    let view = ws
        .read_with(vcx, |ws, cx| ws.open_view(row.id, cx))
        .expect("resumed view opened");
    let shown = view.read_with(vcx, |v, _| {
        v.items.iter().any(|i| {
            matches!(i, super::thread_model::Item::Prompt(p) if p.contains("SkyeLab restarted and resumed this turn"))
        })
    });
    assert!(shown, "timeline shows the restart continuation prompt");
}

#[gpui::test]
fn second_interruption_on_resumed_turn_is_not_resumed_again(cx: &mut TestAppContext) {
    setup(cx);
    let mut f = fixture(true);
    let db = on_disk(&mut f);
    let settings = f._home.path().join("settings.toml");
    cx.update(|cx| resume_after_restart::install(Some(settings), cx));

    let (row, _ws, _vcx) = stopped_thread(&f, cx);
    // Simulate a thread that was ALREADY auto-resumed once and then crashed again mid-turn.
    append_record(&f, row.id, &Record::Prompt(RESTART_RESUME_PROMPT.into()));
    f.store
        .lock()
        .expect("lock")
        .set_thread_status(row.id, "running")
        .expect("mark running");

    reopen_store(&mut f, &db);
    let (_ws, vcx) = open_workspace(&f, cx);
    vcx.run_until_parked();
    assert_eq!(
        env_reports(&f.store, row.id).len(),
        1,
        "continuation chain capped at 1: must not launch a second auto-resume"
    );
    assert_eq!(
        restart_prompts(&f, row.id),
        1,
        "no second restart prompt appended"
    );
}

#[gpui::test]
fn pending_approval_question_plan_and_no_session_are_not_resumed(cx: &mut TestAppContext) {
    setup(cx);
    let mut f = fixture(true);
    let db = on_disk(&mut f);
    let settings = f._home.path().join("settings.toml");
    cx.update(|cx| resume_after_restart::install(Some(settings), cx));

    let project_id = f
        .store
        .lock()
        .expect("lock")
        .create_project("demo", &f.project_dir)
        .expect("project")
        .id;
    let make = |title: &str, session: Option<&str>, tool: Option<&str>| -> i64 {
        let store = f.store.lock().expect("lock");
        let t = store
            .create_thread(&NewThread {
                project_id,
                title,
                cwd: &f.project_dir,
                account_id: "acme",
                status: "running",
            })
            .expect("thread");
        if let Some(s) = session {
            store.set_agent_session_id(t.id, Some(s)).expect("session");
        }
        drop(store);
        append_record(&f, t.id, &Record::Prompt("do it".into()));
        if let Some(tool) = tool {
            append_record(
                &f,
                t.id,
                &Record::Agent(Event::Permission {
                    id: "req-1".into(),
                    tool: tool.into(),
                    input: serde_json::json!({}),
                }),
            );
        }
        t.id
    };
    let perm_id = make("waiting perm", Some("fake-sess-1"), Some("Bash"));
    let question_id = make("waiting question", Some("fake-sess-1"), Some(QUESTION_TOOL));
    let plan_id = make("waiting plan", Some("fake-sess-1"), Some(PLAN_TOOL));
    let no_sess_id = make("no session yet", None, None);

    reopen_store(&mut f, &db);
    let (_ws, vcx) = open_workspace(&f, cx);
    vcx.run_until_parked();
    for id in [perm_id, question_id, plan_id, no_sess_id] {
        assert_eq!(restart_prompts(&f, id), 0, "thread {id} must not auto-resume");
        assert!(env_reports(&f.store, id).is_empty(), "no agent for {id}");
    }
}

#[gpui::test]
fn setting_off_disables_auto_resume(cx: &mut TestAppContext) {
    setup(cx);
    let mut f = fixture(true);
    let db = on_disk(&mut f);
    let settings = f._home.path().join("settings.toml");
    AppSettings {
        resume_after_restart: false,
        ..AppSettings::default()
    }
    .save(&settings)
    .expect("save settings");
    cx.update(|cx| resume_after_restart::install(Some(settings), cx));

    let (row, _ws, _vcx) = stopped_thread(&f, cx);
    append_record(&f, row.id, &Record::Prompt("keep going".into()));
    f.store
        .lock()
        .expect("lock")
        .set_thread_status(row.id, "running")
        .expect("mark running");

    reopen_store(&mut f, &db);
    let (_ws, vcx) = open_workspace(&f, cx);
    vcx.run_until_parked();
    assert_eq!(restart_prompts(&f, row.id), 0, "setting off: no auto-resume");
    assert_eq!(env_reports(&f.store, row.id).len(), 1, "no second launch");
}

#[gpui::test]
fn restore_defaults_resets_resume_after_restart_to_on(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
    });
    let tmp = tempfile::tempdir().expect("tempdir");
    let settings = tmp.path().join("settings.toml");
    cx.update(|cx| {
        super::quit_guard::install(Some(settings.clone()), cx);
        resume_after_restart::install(Some(settings.clone()), cx);
        resume_after_restart::set_enabled(false, cx).expect("save off");
    });
    assert!(!cx.update(|cx| resume_after_restart::enabled(cx)));
    assert!(
        !AppSettings::load(&settings)
            .expect("load")
            .resume_after_restart
    );

    let accounts = tmp.path().join("accounts.toml");
    let (view, vcx) = cx.add_window_view(|window, cx| {
        Settings::new(Some(accounts), AccountsConfig::default(), None, window, cx)
    });
    view.update_in(vcx, |v, window, cx| {
        v.section = Section::General;
        v.restore_defaults(window, cx);
    });
    vcx.run_until_parked();
    assert!(
        vcx.update(|_, cx| resume_after_restart::enabled(cx)),
        "Restore defaults turns resume_after_restart back on"
    );
    assert!(
        AppSettings::load(&settings)
            .expect("load")
            .resume_after_restart
    );
}
