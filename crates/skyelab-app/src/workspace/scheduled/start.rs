//! A scheduled run's thread: started like a delegated one (a background tab on the hub window), under the account
//! the folder resolves to now, and only if that is the one the task was saved under.

use super::super::Workspace;
use super::super::thread::ThreadView;
use gpui::{AppContext as _, Context, Entity, Window};
use skyelab_core::AccountsConfig;
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Entity<ThreadView>, Refused> {
        check_mode(&task.permission_mode, task.bypass_confirmed).map_err(Refused::Pause)?;
        let project = self
            .projects
            .iter()
            .find(|p| p.id == task.project_id)
            .cloned()
            .ok_or_else(|| Refused::Pause("its project was removed from SkyeLab".into()))?;
        // The file as it is now, not this window's copy; one that can't be read binds nothing.
        let config = self
            .accounts_path
            .as_deref()
            .and_then(|p| AccountsConfig::load(p).ok())
            .unwrap_or_default();
        let account = run_account(&config, &project.path, &task.account_id)
            .map_err(Refused::Pause)?
            .id
            .clone();
        let backend = self.backend.clone();
        let view = cx.new(|cx| {
            let mut v = ThreadView::new(&project, config, backend, window, cx);
            v.wt.use_worktree = task.worktree;
            v.mode = skyelab_agent::Mode::parse(&task.permission_mode);
            // The launch refuses if the folder is rebound while it runs (a worktree being made).
            v.required_account = Some(account);
            v.scheduled_task = Some(task.id);
            if task.model.is_some() {
                v.composer.pick = ModelChoice {
                    model: task.model.clone(),
                    effort: task.effort.clone(),
                };
                v.composer.picked = true;
            }
            v
        });
        view.update(cx, |v, cx| v.submit_remote(&task.prompt, window, cx))
            .map_err(Refused::Fail)?;
        self.add_thread_tab(view.clone(), false, window, cx);
        Ok(view)
    }
}
