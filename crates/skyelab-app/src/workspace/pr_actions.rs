//! Acting on a pull request: merge it, or review it (approve, request changes, comment), in a small dialog opened
//! from the details card's PR row or a Pull requests page row. After T3's `PullRequestDetailActions` (the merge button
//! names its method, a click arms it and a second click merges) and `PullRequestReviewForm` (a summary box and the
//! three verdicts). gh runs with the account the folder is bound to now, checked against the account the caller
//! showed, never the user's default gh login.

use super::usage_page_render::segments;
use super::worktree::resolves_to;
use crate::ui;
use crate::ui::SkyeIcon;
use crate::ui::{Button, Checkbox, TextField};
use anyhow::bail;
use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, Render, SharedString, Task, Window, div,
    prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, Sizable as _, WindowExt as _, h_flex,
    input::{InputEvent, InputState},
    v_flex,
};
use skyelab_core::AccountsConfig;
use skyelab_git::{Checks, GitHub, MergeMethod, ReviewComment, Verdict};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// The PR and who acts on it.
#[derive(Clone, PartialEq)]
pub(crate) struct Target {
    /// A canonical GitHub PR link.
    pub url: String,
    pub number: u64,
    /// `None` when the PR wasn't read yet: merging then asks once more, as for failing checks.
    pub checks: Option<Checks>,
    /// The head commit of the same read as `checks`: the merge is pinned to it. `None` refuses the merge.
    pub head: Option<String>,
    /// The folder whose binding picks the account.
    pub dir: PathBuf,
    /// The account the caller showed (the thread's, or the page group's): the folder must still be bound to it.
    pub account: String,
    pub config: AccountsConfig,
}

/// What runs after an action succeeds (the caller reads the PR again); `true` after a review.
type OnDone = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// Where the merge button is: each click moves it one step, the last one merges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    Idle,
    Confirm,
    /// The checks are failing (or unknown): one more click.
    ConfirmFailing,
}

enum Act {
    /// The head commit to pin, the method, and whether to delete the branch.
    Merge(String, MergeMethod, bool),
    /// The verdict, the summary and the line comments (none: `gh pr review`).
    Review(Verdict, String, Vec<ReviewComment>),
}

pub(crate) struct PrActions {
    pub(crate) target: Target,
    gh: PathBuf,
    accounts_file: Option<PathBuf>,
    /// The repo's merge methods; `None` while they are read.
    pub(crate) methods: Option<Result<Vec<MergeMethod>, SharedString>>,
    pub(crate) method: MergeMethod,
    pub(crate) delete_branch: bool,
    pub(crate) stage: Stage,
    pub(crate) busy: bool,
    /// The running action is the merge: its button says so.
    pub(crate) merging: bool,
    pub(crate) error: Option<SharedString>,
    pub(crate) body: Entity<InputState>,
    /// Line comments from the PR's diff tab, sent with the review.
    pub(crate) comments: Vec<ReviewComment>,
    /// Opened from the PR's diff tab: no "View diff".
    pub(crate) from_diff: bool,
    on_done: OnDone,
    _read: Task<()>,
}

/// The bound account's gh config dir for `dir`, read from `config` (accounts.toml as saved now): the folder must
/// still be bound to `account`, and that account signed in to GitHub.
pub(super) fn account_gh_dir(
    config: &AccountsConfig,
    dir: &Path,
    account: &str,
) -> anyhow::Result<PathBuf> {
    resolves_to(config, dir, account)?;
    let gh_dir = config.github_config_dir(dir)?;
    if !skyelab_git::gh_logged_in(&gh_dir) {
        bail!("{account} isn't signed in to GitHub. Sign in under Settings, Source control.");
    }
    Ok(gh_dir)
}

/// Opens the dialog for `target`; `on_done` runs after an action succeeds (the dialog is closed by then).
pub(crate) fn open(
    target: Target,
    on_done: impl Fn(bool, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PrActions> {
    let gh = super::gh_bin(cx);
    let accounts_file = cx.try_global::<super::AccountsFile>().map(|f| f.0.clone());
    let entity = cx.new(|cx| {
        let body = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Leave a comment (needed to comment or request changes)")
        });
        // Comment and Request changes turn on as soon as there is text.
        cx.subscribe(&body, |_, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach(); // both die with the dialog
        let read = {
            let (target, gh, file) = (target.clone(), gh.clone(), accounts_file.clone());
            let methods = cx.background_spawn(async move {
                let config = super::accounts_now(file.as_deref(), &target.config);
                let gh_dir = account_gh_dir(&config, &target.dir, &target.account)?;
                let github = GitHub {
                    gh: &gh,
                    config_dir: &gh_dir,
                };
                anyhow::Ok(skyelab_git::merge_methods(&github, &target.url)?)
            });
            cx.spawn(async move |this, cx| {
                let methods = methods.await;
                let _ = this.update(cx, |this: &mut PrActions, cx| {
                    if let Ok(m) = &methods
                        && let Some(&first) = m.first()
                    {
                        this.method = first;
                    }
                    this.methods = Some(methods.map_err(|e| format!("{e:#}").into()));
                    cx.notify();
                });
            })
        };
        PrActions {
            target: target.clone(),
            gh,
            accounts_file,
            methods: None,
            method: MergeMethod::Squash,
            delete_branch: false,
            stage: Stage::Idle,
            busy: false,
            merging: false,
            error: None,
            body,
            comments: Vec::new(),
            from_diff: false,
            on_done: Rc::new(on_done),
            _read: read,
        }
    });
    let title: SharedString = format!("Pull request #{}", target.number).into();
    let view = entity.clone();
    crate::ui::open_dialog(window, cx, move |dialog, _, _| {
        dialog
            .w(px(480.))
            .title(title.clone())
            .child(view.clone())
            .footer(ui::dialog_footer().child(ui::dialog_cancel("pr-actions-cancel")))
    });
    entity
}

impl PrActions {
    /// T3's labels for the merge button.
    fn merge_label(method: MergeMethod) -> &'static str {
        match method {
            MergeMethod::Squash => "Squash and merge",
            MergeMethod::Merge => "Create a merge commit",
            MergeMethod::Rebase => "Rebase and merge",
        }
    }

    pub(crate) fn pick_method(&mut self, method: MergeMethod, cx: &mut Context<Self>) {
        self.method = method;
        self.stage = Stage::Idle;
        cx.notify();
    }

    /// The merge button: arm, then confirm, then (checks failing) confirm again, then merge.
    pub(crate) fn press_merge(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let allowed = matches!(&self.methods, Some(Ok(m)) if m.contains(&self.method));
        if self.busy || !allowed {
            return;
        }
        self.stage = match (self.stage, self.target.checks) {
            (Stage::Idle, _) => Stage::Confirm,
            (Stage::Confirm, Some(Checks::Failing) | None) => Stage::ConfirmFailing,
            _ => {
                self.stage = Stage::Idle;
                let Some(head) = self.target.head.clone() else {
                    self.error = Some(
                        "This pull request's head commit isn't known, so the merge can't be pinned to the \
                         checks shown. Refresh it, then merge."
                            .into(),
                    );
                    return cx.notify();
                };
                return self.run(
                    Act::Merge(head, self.method, self.delete_branch),
                    window,
                    cx,
                );
            }
        };
        cx.notify();
    }

    pub(crate) fn review(&mut self, verdict: Verdict, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.body.read(cx).value().trim().to_owned();
        if self.busy || !self.can_review(verdict, &body) {
            return;
        }
        self.run(
            Act::Review(verdict, body, self.comments.clone()),
            window,
            cx,
        );
    }

    /// T3's rule: anything but Approve needs a summary or at least one line comment.
    fn can_review(&self, verdict: Verdict, body: &str) -> bool {
        verdict == Verdict::Approve || !body.trim().is_empty() || !self.comments.is_empty()
    }

    /// "View diff": the PR's diff tab, through the workspace (the dialog closes first, so the action reaches it).
    pub(crate) fn view_diff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.target.clone();
        window.close_dialog(cx);
        crate::workspace::dispatch(
            Box::new(super::changes::pr::OpenPrDiff { target }),
            window,
            cx,
        );
    }

    fn run(&mut self, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        (self.busy, self.error) = (true, None);
        self.merging = matches!(act, Act::Merge(..));
        let (target, gh, file) = (
            self.target.clone(),
            self.gh.clone(),
            self.accounts_file.clone(),
        );
        let reviewed = matches!(act, Act::Review(..));
        let number = self.target.number;
        let done = cx.background_spawn(async move {
            let config = super::accounts_now(file.as_deref(), &target.config);
            let gh_dir = account_gh_dir(&config, &target.dir, &target.account)?;
            let github = GitHub {
                gh: &gh,
                config_dir: &gh_dir,
            };
            match act {
                Act::Merge(head, method, delete) => {
                    skyelab_git::merge_pr(&github, &target.url, &head, method, delete)?
                }
                Act::Review(verdict, body, comments) if comments.is_empty() => {
                    skyelab_git::review_pr(&github, &target.url, verdict, &body)?
                }
                Act::Review(verdict, body, comments) => {
                    // Pinned to the commit the diff tab read: GitHub places each comment on that commit's lines.
                    let head = target.head.as_deref().unwrap_or_default();
                    skyelab_git::submit_review(
                        &github,
                        &target.url,
                        head,
                        verdict,
                        &body,
                        &comments,
                    )?
                }
            }
            anyhow::Ok(())
        });
        let on_done = self.on_done.clone();
        cx.spawn_in(window, async move |this, cx| {
            let done = done.await;
            let ok = done.is_ok();
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match done {
                    Err(e) => this.error = Some(format!("{e:#}").into()),
                    Ok(()) => window.close_dialog(cx),
                }
                cx.notify();
            });
            // Also when the dialog was cancelled while gh ran: the PR changed all the same.
            if ok {
                let _ = cx.update(|window, cx| {
                    let note = if reviewed {
                        format!("Review sent on pull request #{number}")
                    } else {
                        format!("Merged pull request #{number}")
                    };
                    ui::toast::push(window, ui::toast::Toast::success(note), cx);
                    on_done(reviewed, window, cx)
                });
            }
        })
        .detach(); // gh is killed at its timeout; `this` is weak
        cx.notify();
    }

    fn render_merge(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = |text: SharedString| {
            div()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(text)
        };
        let methods = match &self.methods {
            None => muted("Reading the repository's merge options…".into()).into_any_element(),
            Some(Err(e)) => div()
                .text_size(ui::META)
                .text_color(ui::ink(theme, theme.danger))
                .child(format!("Couldn't read the merge options: {e}"))
                .into_any_element(),
            Some(Ok(m)) if m.is_empty() => {
                muted("This repository allows no merge method.".into()).into_any_element()
            }
            Some(Ok(m)) => {
                let items: Vec<(MergeMethod, &'static str)> = m
                    .iter()
                    .map(|&m| {
                        let label = match m {
                            MergeMethod::Squash => "Squash",
                            MergeMethod::Merge => "Merge commit",
                            MergeMethod::Rebase => "Rebase",
                        };
                        (m, label)
                    })
                    .collect();
                // Content width, like the page's filter tabs.
                h_flex()
                    .child(segments(
                        "pr-merge-method",
                        &items,
                        self.method,
                        |this: &mut Self, m, cx| this.pick_method(m, cx),
                        cx,
                    ))
                    .into_any_element()
            }
        };
        let theme = cx.theme();
        let warn = match (self.stage, self.target.checks) {
            (Stage::ConfirmFailing, None) => Some("The checks weren't read yet. Merge anyway?"),
            (Stage::ConfirmFailing, _) => Some("Checks are failing. Merge anyway?"),
            (_, Some(Checks::Failing)) => Some("Checks are failing."),
            _ => None,
        };
        let label = match self.stage {
            _ if self.busy && self.merging => "Merging…",
            Stage::Idle => Self::merge_label(self.method),
            Stage::Confirm => "Confirm merge",
            Stage::ConfirmFailing => "Merge anyway",
        };
        let ready = matches!(&self.methods, Some(Ok(m)) if m.contains(&self.method));
        let button = Button::new("pr-merge")
            .small()
            .icon(SkyeIcon::GitMerge)
            .label(label)
            .disabled(self.busy || !ready)
            .loading(self.busy)
            .on_click(cx.listener(|this, _, window, cx| this.press_merge(window, cx)));
        let button = if self.stage == Stage::ConfirmFailing {
            button.danger()
        } else {
            button.primary()
        };
        let delete = self.delete_branch;
        v_flex()
            .gap_2()
            .child(section("Merge", theme))
            .child(methods)
            .child(
                Checkbox::new("pr-delete-branch")
                    .xsmall()
                    .label("Delete the branch on GitHub")
                    .checked(delete)
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        this.delete_branch = *on;
                        cx.notify();
                    })),
            )
            .child(
                h_flex()
                    .gap_2()
                    .children(warn.map(|w| {
                        h_flex()
                            .id("pr-checks-failing")
                            .gap_1()
                            .text_size(ui::META)
                            .text_color(ui::ink(theme, theme.danger))
                            .child(Icon::new(SkyeIcon::CircleX).xsmall())
                            .child(w)
                    }))
                    .child(div().ml_auto().child(button)),
            )
    }

    fn render_review(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let body = self.body.read(cx).value().to_string();
        let verdict = |id: &'static str, label: &'static str, v: Verdict| {
            Button::new(id)
                .small()
                .label(label)
                .disabled(self.busy || !self.can_review(v, &body))
                .on_click(cx.listener(move |this, _, window, cx| this.review(v, window, cx)))
        };
        v_flex()
            .gap_2()
            .child(section("Review", theme))
            .children((!self.comments.is_empty()).then(|| {
                div()
                    .id("pr-review-line-comments")
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(match self.comments.len() {
                        1 => "1 line comment goes with this review.".to_owned(),
                        n => format!("{n} line comments go with this review."),
                    })
            }))
            .child(TextField::new(&self.body).small().disabled(self.busy))
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(verdict("pr-review-comment", "Comment", Verdict::Comment).outline())
                    .child(
                        verdict(
                            "pr-review-changes",
                            "Request changes",
                            Verdict::RequestChanges,
                        )
                        .outline(),
                    )
                    .child(
                        verdict("pr-review-approve", "Approve", Verdict::Approve)
                            .outline()
                            .icon(SkyeIcon::Check),
                    ),
            )
    }
}

fn section(title: &'static str, theme: &gpui_component::Theme) -> impl IntoElement {
    div()
        .text_size(ui::META)
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme.muted_foreground)
        .child(title)
}

impl Render for PrActions {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let link = self
            .target
            .url
            .trim_start_matches("https://github.com/")
            .to_owned();
        let error = self.error.clone().map(|e| {
            div()
                .id("pr-actions-error")
                .text_size(ui::META)
                .text_color(ui::ink(theme, theme.danger))
                .child(e)
        });
        let link = ui::mono(link, theme).text_size(ui::META);
        let view_diff = (!self.from_diff).then(|| {
            Button::new("pr-view-diff")
                .ghost()
                .xsmall()
                .icon(SkyeIcon::FileDiff)
                .label("View diff")
                .on_click(cx.listener(|this, _, window, cx| this.view_diff(window, cx)))
        });
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().min_w_0().truncate().child(link))
                    .children(view_diff),
            )
            .child(self.render_merge(cx))
            .child(div().h(px(1.)).bg(cx.theme().border))
            .child(self.render_review(cx))
            .children(error)
    }
}

#[cfg(all(test, unix))]
#[path = "pr_diff_tab_test.rs"]
mod diff_tab_tests;
#[cfg(all(test, unix))] // the fake gh is a shell script
#[path = "pr_actions_test.rs"]
pub(super) mod tests;
