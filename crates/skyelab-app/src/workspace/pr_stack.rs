//! Stacked pull requests in the app: the stacks last read per PR link (the "Stack 2/3" badges on Pull requests page
//! rows and the details card's PR row) and the PR actions dialog's own read, badge and **Merge stack**, which merges
//! the open layers bottom first with the account the folder is bound to, never the default gh login.

use super::pr_actions::{PrActions, Stage, Target, account_gh_dir};
use crate::ui;
use crate::ui::ActiveTheme as _;
use crate::ui::{Button, SkyeIcon};
use gpui::{
    App, AppContext as _, Context, Global, IntoElement, SharedString, Task, Window, div, prelude::*,
};
use gpui_component::{Icon, Sizable as _, h_flex, v_flex};
use skyelab_git::{Checks, GitHub, PrRow, PrStack};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The stack of each open PR (`None`: it stands alone) as its repo's whole open list was last read, by PR link.
#[derive(Default)]
pub(crate) struct PrStacks(HashMap<String, Option<PrStack>>);

impl Global for PrStacks {}

impl PrStacks {
    pub(crate) fn get<'a>(cx: &'a App, url: &str) -> Option<&'a PrStack> {
        cx.try_global::<Self>()?.0.get(url)?.as_ref()
    }

    /// Whether PR `url` was listed, and its stack then.
    fn known(cx: &App, url: &str) -> Option<Option<PrStack>> {
        cx.try_global::<Self>()?.0.get(url).cloned()
    }

    /// After a merge in the repo of PR `url`: its stacks are unknown until listed again (bases moved, layers merged).
    pub(crate) fn forget_repo(url: &str, cx: &mut App) {
        if let Some(at) = url.rfind("/pull/") {
            let repo = &url[..at + "/pull/".len()];
            cx.default_global::<Self>()
                .0
                .retain(|k, _| !k.starts_with(repo));
        }
    }

    /// `rows`, one repo's open PRs as listed, decide each listed PR's stack (or that it has none now).
    pub(crate) fn record(rows: &[PrRow], cx: &mut App) {
        let map = &mut cx.default_global::<Self>().0;
        for r in rows {
            map.insert(r.url.clone(), skyelab_git::stack_for(r.number, rows));
        }
    }
}

/// A Pull requests page list as it lands: only the unfiltered one holds every open PR, so only it decides stacks.
pub(crate) fn record_listing(
    filter: super::gh_pages::Filter,
    listing: &super::gh_pages::Listing,
    cx: &mut App,
) {
    use super::gh_pages::{Filter, Listing};
    if let (Filter::Pr(skyelab_git::PrFilter::All), Listing::Prs(list)) = (filter, listing) {
        let rows: Vec<PrRow> = list.iter().map(|d| d.row.clone()).collect();
        PrStacks::record(&rows, cx);
    }
}

pub(crate) fn pill(stack: &PrStack) -> ui::Pill {
    ui::Pill::new(stack.badge()).icon(SkyeIcon::GitBranch)
}

/// The badge for PR `url`, while its stack is known.
pub(crate) fn badge(cx: &App, url: &str) -> Option<ui::Pill> {
    PrStacks::get(cx, url).map(pill)
}

/// The dialog's Merge stack: the stack it read itself (with the account it acts as), and the button's step.
pub(crate) struct StackState {
    pub(crate) stack: Option<PrStack>,
    pub(crate) stage: Stage,
    /// The running action is the stack merge: its button says so.
    pub(crate) running: bool,
}

impl Default for StackState {
    fn default() -> Self {
        Self {
            stack: None,
            stage: Stage::Idle,
            running: false,
        }
    }
}

/// The dialog's first read, with the account the folder is bound to now: the repo's merge methods, and its open PRs
/// for the stack unless the Pull requests page listed them (best effort: a failed list only means no stack is offered).
pub(super) fn spawn_load(
    target: &Target,
    gh: &Path,
    file: Option<&Path>,
    cx: &mut Context<PrActions>,
) -> Task<()> {
    let (target, gh, file) = (target.clone(), gh.to_owned(), file.map(PathBuf::from));
    let known = PrStacks::known(cx, &target.url);
    let listed = known.is_some();
    let read = cx.background_spawn(async move {
        let config = super::accounts_now(file.as_deref(), &target.config);
        let gh_dir = account_gh_dir(&config, &target.dir, &target.account)?;
        let github = GitHub {
            gh: &gh,
            config_dir: &gh_dir,
        };
        let methods = skyelab_git::merge_methods(&github, &target.url)?;
        let rows = (!listed).then(|| skyelab_git::repo_open_prs(&github, &target.url).ok());
        anyhow::Ok((methods, rows.flatten()))
    });
    cx.spawn(async move |this, cx| {
        let read = read.await;
        let _ = this.update(cx, |this: &mut PrActions, cx| {
            let methods = read.map(|(methods, rows)| {
                this.stack.stack = match rows {
                    Some(rows) => {
                        PrStacks::record(&rows, cx);
                        skyelab_git::stack_for(this.target.number, &rows)
                    }
                    None => known.flatten(),
                };
                methods
            });
            if let Ok(m) = &methods
                && let Some(&first) = m.first()
            {
                this.method = first;
            }
            this.methods = Some(methods.map_err(|e| format!("{e:#}").into()));
            cx.notify();
        });
    })
}

/// The layers a stack merge takes (the PR and every open one below it), when there is any below.
fn below(stack: &Option<PrStack>) -> Option<&PrStack> {
    stack.as_ref().filter(|s| s.layers.len() > 1)
}

/// Any layer whose listed checks are failing: the merge then asks once more.
fn any_failing(stack: &PrStack) -> bool {
    stack.layers.iter().any(|l| l.checks == Checks::Failing)
}

impl PrActions {
    fn stack_ready(&self) -> bool {
        matches!(&self.methods, Some(Ok(m)) if m.contains(&self.method))
    }

    /// Merge stack: arm, then confirm, then (a layer's checks failing) confirm again, then merge bottom first.
    pub(crate) fn press_merge_stack(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(stack) = below(&self.stack.stack).cloned() else {
            return;
        };
        if self.busy || !self.stack_ready() {
            return;
        }
        self.stack.stage = match self.stack.stage {
            Stage::Idle => Stage::Confirm,
            Stage::Confirm if any_failing(&stack) => Stage::ConfirmFailing,
            _ => {
                self.stack.stage = Stage::Idle;
                if let Some(l) = stack.layers.iter().find(|l| l.head.is_none()) {
                    self.error = Some(
                        format!(
                            "#{}'s head commit isn't known, so the stack can't be pinned to the checks shown. \
                             Refresh it, then merge.",
                            l.number
                        )
                        .into(),
                    );
                    return cx.notify();
                }
                return self.run_merge_stack(stack, window, cx);
            }
        };
        cx.notify();
    }

    fn run_merge_stack(&mut self, stack: PrStack, window: &mut Window, cx: &mut Context<Self>) {
        (self.busy, self.merging, self.stack.running, self.error) = (true, false, true, None);
        let (target, gh, file) = (
            self.target.clone(),
            self.gh.clone(),
            self.accounts_file.clone(),
        );
        let (method, delete, total) = (self.method, self.delete_branch, stack.layers.len());
        let done = cx.background_spawn(async move {
            // The binding as saved now, right before gh runs: a rebind since the dialog opened refuses.
            let config = super::accounts_now(file.as_deref(), &target.config);
            let gh_dir = account_gh_dir(&config, &target.dir, &target.account)?;
            let github = GitHub {
                gh: &gh,
                config_dir: &gh_dir,
            };
            anyhow::Ok(skyelab_git::merge_stack(&github, &stack, method, delete))
        });
        let (on_done, stack_url) = (self.on_done.clone(), self.target.url.clone());
        cx.spawn_in(window, async move |this, cx| {
            let (merged, error) = match done.await {
                Err(e) => (0, Some(format!("{e:#}"))),
                Ok(Ok(merged)) => (merged.len(), None),
                Ok(Err(stopped)) => (stopped.merged.len(), Some(stopped.to_string())),
            };
            let _ = this.update_in(cx, |this, window, cx| {
                (this.busy, this.stack.running) = (false, false);
                match &error {
                    // What merged is gone from GitHub's stack: offer no stale stack again; reopening lists it anew.
                    Some(e) => (this.error, this.stack.stack) = (Some(e.clone().into()), None),
                    None => crate::ui::close_dialog(window, cx),
                }
                cx.notify();
            });
            let _ = cx.update(|window, cx| {
                if merged > 0 {
                    let note = format!("Merged {merged} of {total} stacked pull requests");
                    ui::toast::push(window, ui::toast::Toast::success(note), cx);
                    on_done(false, window, cx); // reads every layer again, from the stack still known
                }
                PrStacks::forget_repo(&stack_url, cx);
            });
        })
        .detach(); // gh is killed at its timeout; `this` is weak
        cx.notify();
    }

    pub(super) fn render_stack_badge(&self) -> Option<impl IntoElement> {
        self.stack
            .stack
            .as_ref()
            .map(|s| div().id("pr-stack-badge").child(pill(s)))
    }

    /// "Merge stack (N PRs)" under the single merge, when the PR has open layers below; `None` otherwise.
    pub(super) fn render_merge_stack(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let stack = below(&self.stack.stack)?;
        let theme = cx.theme();
        let n = stack.layers.len();
        let order: Vec<String> = stack
            .layers
            .iter()
            .map(|l| format!("#{}", l.number))
            .collect();
        let note = format!(
            "Merges {} into {}, bottom first. Each PR above the bottom is moved onto {} right before it merges; it \
             stops at the first that fails.",
            order.join(", "),
            stack.onto(),
            stack.onto()
        );
        let warn = match self.stack.stage {
            Stage::ConfirmFailing => Some("A layer's checks are failing. Merge the stack anyway?"),
            _ if any_failing(stack) => Some("A layer's checks are failing."),
            _ => None,
        };
        let label: SharedString = match self.stack.stage {
            _ if self.stack.running => "Merging the stack…".into(),
            Stage::Idle => format!("Merge stack ({n} PRs)").into(),
            Stage::Confirm => format!("Confirm merging {n} PRs").into(),
            Stage::ConfirmFailing => "Merge stack anyway".into(),
        };
        let button = Button::new("pr-merge-stack")
            .small()
            .icon(SkyeIcon::GitMerge)
            .label(label)
            .disabled(self.busy || !self.stack_ready())
            .loading(self.stack.running)
            .on_click(cx.listener(|this, _, window, cx| this.press_merge_stack(window, cx)));
        let button = if self.stack.stage == Stage::ConfirmFailing {
            button.danger()
        } else {
            button.outline()
        };
        Some(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .id("pr-merge-stack-note")
                        .text_size(ui::META)
                        .text_color(theme.muted_foreground)
                        .child(note),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .children(warn.map(|w| {
                            h_flex()
                                .id("pr-stack-checks-failing")
                                .gap_1()
                                .text_size(ui::META)
                                .text_color(ui::ink(theme, theme.danger))
                                .child(Icon::new(SkyeIcon::CircleX).xsmall())
                                .child(w)
                        }))
                        .child(div().ml_auto().child(button)),
                ),
        )
    }
}

#[cfg(test)]
mod unit {
    use super::*;
    use skyelab_git::{PrState, Review};

    fn row(number: u64, head_ref: &str, base_ref: &str, checks: Checks) -> PrRow {
        PrRow {
            number,
            title: String::new(),
            url: format!("https://github.com/acme/demo/pull/{number}"),
            author: String::new(),
            draft: false,
            checks,
            review: Review::None,
            updated_at: String::new(),
            head: None,
            head_ref: head_ref.into(),
            base_ref: base_ref.into(),
            state: PrState::Open,
        }
    }

    #[test]
    fn pr_stack_only_offers_a_merge_with_layers_below_and_warns_on_any_failing_layer() {
        let rows = [
            row(1, "a", "main", Checks::Passing),
            row(2, "b", "a", Checks::Failing),
            row(3, "c", "b", Checks::Passing),
        ];
        let bottom = skyelab_git::stack_for(1, &rows);
        assert_eq!(
            bottom.as_ref().map(PrStack::badge).as_deref(),
            Some("Stack 1/3")
        );
        assert!(below(&bottom).is_none(), "nothing below the bottom");
        let top = skyelab_git::stack_for(3, &rows);
        assert!(any_failing(below(&top).expect("two below")));
        let middle = skyelab_git::stack_for(2, &rows[..2]).expect("stacked");
        assert_eq!(middle.badge(), "Stack 2/2");
        assert!(any_failing(&middle));
        let passing = [rows[0].clone(), row(2, "b", "a", Checks::Pending)];
        assert!(!any_failing(
            &skyelab_git::stack_for(2, &passing).expect("stacked")
        ));
    }
}

#[cfg(all(test, unix))] // the fake gh is a shell script
#[path = "pr_stack_test.rs"]
mod tests;
