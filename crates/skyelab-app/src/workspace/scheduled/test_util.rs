//! Scheduled-task test rig: a stored project bound (or not) to "acme", a workspace on it, and the fake clock.

use super::super::threads_test::{Fixture, fixture, open_workspace, setup};
use super::super::{SharedStore, Workspace, lock};
use gpui::{Entity, TestAppContext, VisualTestContext};
use skyelab_core::{Account, AccountsConfig, Binding, ScheduledSettings};
use skyelab_store::Thread;
use skyelab_store::scheduled::ScheduledTask;

/// The fake clock's "now": every test ticks at a fixed time.
pub(super) const NOW: i64 = 1_800_000_000_000;
pub(super) const HOUR: i64 = 3_600_000;

pub(super) struct Rig {
    pub f: Fixture,
    pub project: i64,
}

pub(super) fn rig(bound: bool) -> Rig {
    let f = fixture(bound);
    let project = lock(&f.store)
        .expect("store")
        .create_project("demo", &f.project_dir)
        .expect("project")
        .id;
    Rig { f, project }
}

/// Opens the rig's workspace with the fake clock at [`NOW`].
pub(super) fn open<'a>(
    rig: &Rig,
    cx: &'a mut TestAppContext,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    setup(cx);
    let (ws, vcx) = open_workspace(&rig.f, cx);
    vcx.update(|_, cx| super::set_now(NOW, cx));
    (ws, vcx)
}

/// A task in the rig's project, saved under "acme", due at `next`.
pub(super) fn task(rig: &Rig, schedule: &str, next: i64) -> ScheduledTask {
    let mut t = ScheduledTask {
        project_id: rig.project,
        title: format!("task {schedule}"),
        prompt: "scheduled hello".into(),
        schedule: schedule.into(),
        permission_mode: "default".into(),
        account_id: "acme".into(),
        enabled: true,
        next_run_at: Some(next),
        ..Default::default()
    };
    t.id = lock(&rig.f.store)
        .expect("store")
        .save_scheduled_task(&t)
        .expect("save task");
    t
}

/// Ticks from the app, outside any window's update, as the real loop does.
pub(super) fn tick(cx: &mut VisualTestContext) {
    TestAppContext::update(cx, super::tick);
}

pub(super) fn set_settings(settings: ScheduledSettings, cx: &mut VisualTestContext) {
    cx.update(|_, cx| super::save_settings(settings, cx).detach());
}

pub(super) fn read(store: &SharedStore, id: i64) -> ScheduledTask {
    lock(store)
        .expect("store")
        .scheduled_task(id)
        .expect("read")
        .expect("task exists")
}

/// The threads task `id` started, oldest first.
pub(super) fn runs(rig: &Rig, id: i64) -> Vec<Thread> {
    let mut threads = lock(&rig.f.store)
        .expect("store")
        .list_threads(rig.project)
        .expect("threads");
    threads.retain(|t| t.scheduled_task_id == Some(id));
    threads.sort_by_key(|t| t.id);
    threads
}

/// A stored thread of task `id` in `status`, with no view behind it (as after a restart, or another window's run).
pub(super) fn stored_run(rig: &Rig, id: i64, status: &str) -> i64 {
    let s = lock(&rig.f.store).expect("store");
    let t = s
        .create_thread(&skyelab_store::NewThread {
            project_id: rig.project,
            title: "an earlier run",
            cwd: &rig.f.project_dir,
            account_id: "acme",
            status,
        })
        .expect("thread");
    s.set_thread_scheduled_task(t.id, id).expect("mark");
    t.id
}

/// Rewrites accounts.toml: the project folder bound to `to` ("home" is added as a second account), or unbound.
pub(super) fn rebind(rig: &Rig, to: Option<&str>) {
    let path = &rig.f.accounts_path;
    let mut config = AccountsConfig::load(path).expect("load");
    if config.accounts.iter().all(|a| a.id != "home") {
        config
            .add_account(Account {
                id: "home".into(),
                label: "Home".into(),
                config_dir: rig.f.account_dir.with_file_name(".claude-home"),
                provider: Default::default(),
                gh_config_dir: None,
                ssh_agent: false,
            })
            .expect("add home");
    }
    config.bindings.retain(|b| b.path != rig.f.project_dir);
    if let Some(id) = to {
        config
            .add_binding(Binding::new(rig.f.project_dir.clone(), id.into()))
            .expect("bind");
    }
    config.save(path).expect("save");
}
