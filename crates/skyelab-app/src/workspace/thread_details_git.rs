//! The details card's version control rows, Lineage and Pull request sections.

use super::super::OpenThread;
use super::super::thread::{ChildLink, ThreadView};
use super::super::thread::{GitAction, ThreadViewEvent};
use super::super::thread_worktree::{DiscardWorktree, OpenChanges, OpenWorktreeTerminal};
use super::card::{chevron, row};
use super::{Page, is_running};
use crate::ui::{self, Status};
use gpui::Focusable as _;
use gpui::{
    AnyElement, AppContext as _, Context, FontWeight, SharedString, Window, div, prelude::*,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit_assets::IconName;

impl ThreadView {
    pub(super) fn render_git_rows(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let thread = self.thread.as_ref();
        let worktree_branch =
            thread.and_then(|t| t.worktree_path.as_ref().and(t.worktree_branch.clone()));
        let picker = self.branch_picker.read(cx);
        let repo = worktree_branch.is_some()
            || !(picker.branches.local.is_empty() && picker.branches.current.is_none());
        if !repo {
            return None;
        }
        let picks_base = thread.is_none() && self.wt.use_worktree;
        let branch_row = match worktree_branch {
            // A worktree thread's branch is its own; it doesn't switch.
            Some(branch) => {
                row("details-branch", IconName::GitBranch, branch, &theme).cursor_default()
            }
            None => {
                let picker = self.branch_picker.clone();
                row(
                    "details-branch",
                    IconName::GitBranch,
                    self.branch_picker.read(cx).label(picks_base),
                    &theme,
                )
                .child(chevron(IconName::ChevronDown, &theme))
                .on_click(cx.listener(move |_, _, _, cx| {
                    picker.update(cx, |p, cx| p.set_open(true, cx));
                    cx.notify();
                }))
            }
        };
        let base = thread
            .and_then(|t| t.worktree_base.as_deref())
            .map_or("HEAD".to_owned(), |b| b.chars().take(7).collect());
        let stat = self.details.stat.map(|(added, removed)| {
            h_flex()
                .flex_shrink_0()
                .gap_1p5()
                .font_family(theme.mono_font_family.clone())
                .text_size(ui::META)
                .child(
                    div()
                        .text_color(ui::ink(&theme, theme.success))
                        .child(format!("+{added}")),
                )
                .child(
                    div()
                        .text_color(ui::ink(&theme, theme.danger))
                        .child(format!("−{removed}")),
                )
        });
        let has_worktree = thread.is_some_and(|t| t.worktree_path.is_some());
        Some(
            v_flex()
                .child(branch_row)
                .child(
                    row(
                        "details-commit",
                        IconName::CloudUpload,
                        "Commit & push",
                        &theme,
                    )
                    .child(chevron(IconName::ChevronRight, &theme))
                    .on_click(cx.listener(|this, _, _, cx| this.open_git_menu(cx))),
                )
                .child(
                    row(
                        "details-changes",
                        IconName::FileDiff,
                        format!("Changes vs {base}"),
                        &theme,
                    )
                    .children(stat)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.details_home(cx);
                        this.open_changes(&OpenChanges, window, cx);
                    })),
                )
                .when(has_worktree, |v| {
                    v.child(
                        row(
                            "details-worktree-terminal",
                            IconName::SquareTerminal,
                            "Open terminal here",
                            &theme,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.details_home(cx);
                            this.open_worktree_terminal(&OpenWorktreeTerminal, window, cx);
                        })),
                    )
                    .child(
                        row(
                            "details-discard-worktree",
                            IconName::Trash,
                            "Discard worktree",
                            &theme,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.details_home(cx);
                            this.discard_worktree(&DiscardWorktree, window, cx);
                        })),
                    )
                })
                .into_any_element(),
        )
    }

    /// The running children, then a row to the finished ones; titled with how many run.
    pub(super) fn render_lineage(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<(SharedString, AnyElement)> {
        if self.children.is_empty() {
            return None;
        }
        let theme = cx.theme().clone();
        let running: Vec<&ChildLink> = self
            .children
            .iter()
            .filter(|c| is_running(c.status))
            .collect();
        let previous = self.children.len() - running.len();
        let title = match running.len() {
            0 => "Lineage".into(),
            n => format!("Lineage · {n} running").into(),
        };
        let body = v_flex()
            .children(running.into_iter().map(|c| self.agent_row(c, cx)))
            .when(previous > 0, |v| {
                v.child(
                    row("details-previous", IconName::Bot, "Previous agents", &theme)
                        .child(
                            div()
                                .text_size(ui::META)
                                .text_color(theme.muted_foreground)
                                .child(previous.to_string()),
                        )
                        .child(chevron(IconName::ChevronRight, &theme))
                        .on_click(
                            cx.listener(|this, _, _, cx| this.details_page(Page::Previous, cx)),
                        ),
                )
            });
        Some((title, body.into_any_element()))
    }

    /// The thread's pull request (opened here or linked), else "Link pull request…". `None` before the thread has
    /// a row to keep the link on.
    pub(super) fn render_pr(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let thread = self.thread.as_ref()?;
        let Some(url) = thread.pr_url.clone() else {
            return Some(
                row(
                    "details-link-pr",
                    IconName::Plus,
                    "Link pull request…",
                    &theme,
                )
                .on_click(cx.listener(|this, _, window, cx| this.open_link_pr(window, cx)))
                .into_any_element(),
            );
        };
        let label = url.trim_start_matches("https://").to_owned();
        Some(
            row("details-pr", IconName::GitPullRequest, label, &theme)
                .on_click(move |_, _, cx| {
                    if super::super::thread_render::opens_in_browser(&url) {
                        cx.open_url(&url);
                    }
                })
                .into_any_element(),
        )
    }

    /// The Commit & push menu, and who it pushes as (read from the account's `gh` now, off the UI thread).
    fn open_git_menu(&mut self, cx: &mut Context<Self>) {
        self.details.page = Page::Git;
        self.details.gh_login = None;
        let gh_dir = self.config.github_config_dir(&self.cwd);
        let gh_bin = super::super::gh_bin(cx);
        let task = cx.background_spawn(async move {
            let dir = gh_dir.map_err(|e| format!("{e:#}"))?;
            skyelab_git::gh_login(&skyelab_git::GitHub {
                gh: &gh_bin,
                config_dir: &dir,
            })
            .map_err(|e| format!("{e:#}"))
        });
        self.details._gh = Some(cx.spawn(async move |this, cx| {
            let login = match task.await {
                Ok(Some(login)) => format!("Pushes as @{login}"),
                Ok(None) => "No GitHub login for this account; log in from Settings".into(),
                Err(e) => e,
            };
            let _ = this.update(cx, |this, cx| {
                this.details.gh_login = Some(login.into());
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// T3's GitActionsControl as a page: Commit, Commit & push, Push, then Open PR (View PR once there is one). Each
    /// runs in the Changes tab, where its message and result show.
    pub(super) fn render_git_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let login = self
            .details
            .gh_login
            .clone()
            .unwrap_or_else(|| "Checking the GitHub login…".into());
        let action = |id: &'static str, icon: IconName, label: &'static str, a: GitAction| {
            row(id, icon, label, &theme)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.details_home(cx);
                    cx.emit(ThreadViewEvent::Git(a));
                }))
                .into_any_element()
        };
        let pr = match self.thread.as_ref().and_then(|t| t.pr_url.clone()) {
            Some(url) => row(
                "details-git-view-pr",
                IconName::ExternalLink,
                "View PR",
                &theme,
            )
            .on_click(move |_, _, cx| {
                if super::super::thread_render::opens_in_browser(&url) {
                    cx.open_url(&url);
                }
            })
            .into_any_element(),
            None => action(
                "details-git-open-pr",
                IconName::GitPullRequest,
                "Open PR",
                GitAction::OpenPr,
            ),
        };
        vec![
            div()
                .id("details-git-login")
                .px_2p5()
                .py_1()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(login)
                .into_any_element(),
            action(
                "details-git-commit",
                IconName::GitCommitVertical,
                "Commit",
                GitAction::Commit,
            ),
            action(
                "details-git-commit-push",
                IconName::CloudUpload,
                "Commit & push",
                GitAction::CommitPush,
            ),
            action(
                "details-git-push",
                IconName::ArrowUp,
                "Push",
                GitAction::Push,
            ),
            pr,
        ]
    }

    pub(super) fn open_link_pr(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = match &self.details.pr_input {
            Some(input) => input.clone(),
            None => {
                let input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("https://github.com/owner/repo/pull/123")
                });
                cx.subscribe_in(&input, window, |this, _, e: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter { .. } = e {
                        this.link_pr(window, cx);
                    }
                })
                .detach();
                self.details.pr_input = Some(input.clone());
                input
            }
        };
        self.details.pr_error = None;
        self.details.page = Page::LinkPr;
        input.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// Keeps the typed link as the thread's PR when it is a GitHub pull request URL.
    pub(super) fn link_pr(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(input), Some(id)) = (&self.details.pr_input, self.thread.as_ref().map(|t| t.id))
        else {
            return;
        };
        let typed = input.read(cx).value().to_string();
        let Some(url) = skyelab_core::github_pr_url(&typed) else {
            self.details.pr_error = Some(
                "Paste a GitHub pull request link: https://github.com/owner/repo/pull/123".into(),
            );
            cx.notify();
            return;
        };
        let saved = super::super::lock(&self.backend.store)
            .and_then(|s| s.set_thread_pr_url(id, Some(&url)));
        if let Err(e) = saved {
            self.details.pr_error = Some(format!("Couldn't save the link: {e:#}").into());
            cx.notify();
            return;
        }
        input.update(cx, |s, cx| s.set_value("", window, cx));
        self.set_pr_url(url, cx);
        self.details_home(cx);
    }

    pub(super) fn render_link_pr(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let Some(input) = &self.details.pr_input else {
            return Vec::new();
        };
        vec![
            h_flex()
                .px_2p5()
                .py_1()
                .gap_2()
                .child(div().flex_1().min_w_0().child(Input::new(input).small()))
                .child(
                    Button::new("details-link-pr-save")
                        .small()
                        .primary()
                        .label("Link")
                        .on_click(cx.listener(|this, _, window, cx| this.link_pr(window, cx))),
                )
                .into_any_element(),
        ]
        .into_iter()
        .chain(self.details.pr_error.clone().map(|e| {
            div()
                .id("details-link-pr-error")
                .px_2p5()
                .py_1()
                .text_size(ui::META)
                .text_color(ui::ink(&theme, theme.danger))
                .child(e)
                .into_any_element()
        }))
        .collect()
    }

    /// A delegated thread: its status dot, title, and how long it has run (or how it ended); opens it.
    pub(super) fn agent_row(&self, c: &ChildLink, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let trailing = if is_running(c.status) {
            ui::ago(ui::now_ms(), c.started_ms)
        } else {
            match c.status.badge() {
                Some((_, word)) => word.to_owned(),
                None if c.status == Status::Idle => "Idle".to_owned(),
                None => "Stopped".to_owned(),
            }
        };
        let id = c.id;
        h_flex()
            .id(SharedString::from(format!("details-agent-{id}")))
            .h(ui::ROW_H)
            .px_2p5()
            .gap_2p5()
            .rounded(ui::RADIUS)
            .cursor_pointer()
            .hover(|s| s.bg(theme.accent))
            .text_size(ui::CODE)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.foreground.opacity(0.8))
            .child(
                div()
                    .size_4()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(ui::StatusDot::new(c.status.color(theme))),
            )
            .child(div().flex_1().min_w_0().truncate().child(c.title.clone()))
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(trailing),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.details_home(cx);
                window.dispatch_action(Box::new(OpenThread { id }), cx);
            }))
            .into_any_element()
    }
}
