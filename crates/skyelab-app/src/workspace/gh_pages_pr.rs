//! The Pull requests page's own PR acts: a row's PR as an action target, opening it, its merge and review dialog,
//! and reading that one PR again afterwards.

use super::{GhEvent, GhPage, Listing};
use gpui::{AppContext as _, Context, Entity, Window};
use skyelab_git::GitHub;
use std::sync::Arc;

impl GhPage {
    /// PR `number` of `project`'s shown list as something to act on with the group's account; `None` when it isn't
    /// listed (or its link isn't a canonical PR link).
    fn pr_target(&self, project: i64, number: u64) -> Option<super::super::pr_actions::Target> {
        let source = self.sources.iter().find(|s| s.id == project)?;
        let group = self.groups.get(&(project, self.filter))?;
        let Listing::Prs(list) = &group.listing else {
            return None;
        };
        let pr = &list.iter().find(|r| r.row.number == number)?.row;
        // gh's JSON is untrusted: only a canonical PR link is acted on.
        let url = skyelab_core::github_pr_url(&pr.url)?;
        Some(super::super::pr_actions::Target {
            url,
            number,
            title: Some(pr.title.clone()),
            checks: Some(pr.checks),
            head: pr.head.clone(),
            dir: source.path.clone(),
            account: group.account.as_ref()?.0.clone(),
            config: self.inputs.as_ref()?.config.clone(),
        })
    }

    /// A row's title: the PR itself (its diff tab), as T3's row opens the PR rather than starting anything.
    pub(crate) fn open_pr(&mut self, project: i64, number: u64, cx: &mut Context<Self>) {
        if let Some(target) = self.pr_target(project, number) {
            cx.emit(GhEvent::OpenPr(target));
        }
    }

    /// The merge and review dialog for PR `number` of `project`'s shown list, acting with the group's account; that
    /// PR's row is read again after an action.
    pub(crate) fn open_pr_actions(
        &mut self,
        project: i64,
        number: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<super::super::pr_actions::PrActions>> {
        let target = self.pr_target(project, number)?;
        let page = cx.weak_entity();
        let done = target.clone();
        Some(super::super::pr_actions::open(
            target,
            move |_, _, cx| {
                _ = page.update(cx, |p, cx| p.reread_pr(project, done.clone(), cx));
            },
            window,
            cx,
        ))
    }

    /// Reads PR `target` again with the account that acted and updates its row in every cached list of `project`
    /// (a merged PR stays listed, marked merged, until Refresh). A failed read lists them again.
    fn reread_pr(
        &mut self,
        project: i64,
        target: super::super::pr_actions::Target,
        cx: &mut Context<Self>,
    ) {
        let Some(inputs) = &self.inputs else {
            return;
        };
        let (gh, file) = (inputs.gh.clone(), inputs.accounts_file.clone());
        let account = target.account.clone();
        let url = target.url.clone();
        let read = cx.background_spawn(async move {
            let config = super::super::accounts_now(file.as_deref(), &target.config);
            let gh_dir =
                super::super::pr_actions::account_gh_dir(&config, &target.dir, &target.account)?;
            let github = GitHub {
                gh: &gh,
                config_dir: &gh_dir,
            };
            anyhow::Ok(skyelab_git::pr_status(&github, &target.url)?)
        });
        cx.spawn(async move |this, cx| {
            let status = read.await;
            _ = this.update(cx, |this, cx| {
                let Ok(status) = status else {
                    this.groups.retain(|key, _| key.1 == this.filter);
                    return this.refresh(cx);
                };
                for (key, group) in &mut this.groups {
                    // A list read by another account since (a rebind) keeps its own rows.
                    if key.0 != project || group.account.as_ref().map(|a| &a.0) != Some(&account) {
                        continue;
                    }
                    if let Listing::Prs(list) = &mut group.listing
                        && let Some(at) = list.iter().position(|r| r.row.url == url)
                    {
                        let row = &mut Arc::make_mut(list)[at].row;
                        row.state = status.state;
                        row.draft = status.state == skyelab_git::PrState::Draft;
                        row.checks = status.checks;
                        row.review = status.review;
                        row.head = status.head.clone();
                    }
                }
                cx.notify();
            });
        })
        .detach(); // gh is killed at its timeout; `this` is weak
    }
}
