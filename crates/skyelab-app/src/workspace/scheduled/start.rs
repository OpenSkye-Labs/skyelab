//! A scheduled run's thread: started like a delegated one (a background tab on the hub window), under the account
//! the folder resolves to now, and only if that is the one the task was saved under.

use super::super::center::Page;
use super::super::thread::ThreadView;
use super::super::{Workspace, lock};
use gpui::{AppContext as _, Context, Entity, Window};
use skyelab_core::AccountsConfig;
use skyelab_core::bots::check_trigger_mode;
use skyelab_core::schedule::{check_mode, run_account};
use skyelab_store::ModelChoice;
use skyelab_store::scheduled::ScheduledTask;

/// Why a run didn't start: `Pause` turns the task off until the user fixes it; `Fail` only records this run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::workspace) enum Refused {
    Pause(String),
    Fail(String),
}

impl Workspace {
    pub(super) fn start_scheduled(
        &mut self,
        task: &ScheduledTask,
        tag: super::RunTag,
        prompt: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Entity<ThreadView>, Refused> {
        check_mode(&task.permission_mode, task.bypass_confirmed).map_err(Refused::Pause)?;
        // The store's list, not this window's: a project added or removed in another window counts at once.
        let projects = lock(&self.backend.store)
            .and_then(|s| s.list_projects())
            .map_err(|e| Refused::Fail(format!("{e:#}")))?;
        let project = projects
            .into_iter()
            .find(|p| p.id == task.project_id)
            .ok_or_else(|| Refused::Pause("its project was removed from SkyeLab".into()))?;
        // The file as it is now, not this window's copy; one that can't be read binds nothing.
        let config = self
            .accounts_path
            .as_deref()
            .and_then(|p| AccountsConfig::load(p).ok())
            .unwrap_or_default();
        let account =
            run_account(&config, &project.path, &task.account_id).map_err(Refused::Pause)?;
        // Checked again at every run: a bot GitHub can start runs only in Plan or Ask, whatever the row says.
        check_trigger_mode(
            &task.permission_mode,
            !task.triggers.is_empty(),
            account.provider,
        )
        .map_err(Refused::Pause)?;
        let account = account.id.clone();
        let backend = self.backend.clone();
        let view = cx.new(|cx| {
            let mut v = ThreadView::new(&project, config, backend, window, cx);
            v.wt.use_worktree = task.worktree;
            v.mode = skyelab_agent::Mode::parse(&task.permission_mode);
            // The launch refuses if the folder is rebound while it runs (a worktree being made).
            v.required_account = Some(account);
            v.scheduled_task = Some(tag);
            if task.model.is_some() {
                v.composer.pick = ModelChoice {
                    model: task.model.clone(),
                    effort: task.effort.clone(),
                };
                v.composer.picked = true;
            }
            v
        });
        view.update(cx, |v, cx| v.submit_remote(prompt, window, cx))
            .map_err(Refused::Fail)?;
        self.close_finished_runs(task.id, window, cx);
        self.add_thread_tab(view.clone(), false, window, cx);
        Ok(view)
    }

    /// One live agent per task: its earlier runs whose turn ended close (their rows stay, and resume), or an
    /// every-minute task would keep a new agent process alive per run. The one on screen stays.
    fn close_finished_runs(&mut self, task: i64, window: &mut Window, cx: &mut Context<Self>) {
        let done: Vec<_> = self
            .open_threads
            .iter()
            .filter(|v| {
                let v = v.read(cx);
                v.scheduled_task.map(|r| r.task) == Some(task) && !v.starting && !v.turn_running()
            })
            .filter(|v| self.page != Page::Thread(v.downgrade()))
            .cloned()
            .collect();
        for view in done {
            self.close_thread(&view, window, cx);
        }
    }
}
