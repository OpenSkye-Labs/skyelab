//! The details card's version control rows, Lineage and Pull request sections.

use super::super::OpenThread;
use super::super::thread::{ChildLink, ThreadView};
use super::super::thread_worktree::{DiscardWorktree, OpenChanges, OpenWorktreeTerminal};
use super::card::{chevron, row};
use super::{Page, is_running};
use crate::ui::{self, Status};
use gpui::{AnyElement, Context, FontWeight, SharedString, div, prelude::*};
use gpui_component::{ActiveTheme as _, h_flex, v_flex};
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
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.details_home(cx);
                        this.open_changes(&OpenChanges, window, cx);
                    })),
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

    /// The pull request opened from this thread, if any.
    pub(super) fn render_pr(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let url = self.thread.as_ref()?.pr_url.clone()?;
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
