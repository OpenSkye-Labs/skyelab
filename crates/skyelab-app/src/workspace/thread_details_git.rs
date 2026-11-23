//! The details card's version control, lineage and usage sections.

use super::super::OpenThread;
use super::super::thread::{ChildLink, ThreadView};
use super::super::thread_worktree::{DiscardWorktree, OpenChanges, OpenWorktreeTerminal};
use super::card::{chevron, row};
use super::{Page, is_running};
use crate::ui::{self, Status};
use gpui::{AnyElement, Context, FontWeight, Hsla, SharedString, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, Theme, h_flex, v_flex};
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
                .on_click(move |_, _, cx| picker.update(cx, |p, cx| p.set_open(true, cx)))
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
        let pr = thread.and_then(|t| t.pr_url.clone());
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
                        this.set_details(false, cx);
                        this.open_changes(&OpenChanges, window, cx);
                    })),
                )
                .children(pr.map(|url| {
                    let label = url.trim_start_matches("https://").to_owned();
                    row("details-pr", IconName::GitPullRequest, label, &theme).on_click(
                        move |_, _, cx| {
                            if super::super::thread_render::opens_in_browser(&url) {
                                cx.open_url(&url);
                            }
                        },
                    )
                }))
                .child(
                    row(
                        "details-changes",
                        IconName::FileDiff,
                        format!("Changes vs {base}"),
                        &theme,
                    )
                    .children(stat)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.set_details(false, cx);
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
                            this.set_details(false, cx);
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
                            this.set_details(false, cx);
                            this.discard_worktree(&DiscardWorktree, window, cx);
                        })),
                    )
                })
                .into_any_element(),
        )
    }

    pub(super) fn render_lineage(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
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
            0 => "Lineage".to_owned(),
            n => format!("Lineage · {n} running"),
        };
        Some(
            v_flex()
                .child(
                    div()
                        .h(px(28.))
                        .px_2p5()
                        .flex()
                        .items_center()
                        .text_size(ui::META)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground.opacity(0.8))
                        .child(title),
                )
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
                this.set_details(false, cx);
                window.dispatch_action(Box::new(OpenThread { id }), cx);
            }))
            .into_any_element()
    }

    pub(super) fn render_usage(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.details.limits.is_empty() {
            return None;
        }
        let theme = cx.theme();
        Some(
            v_flex()
                .py_1()
                .children(self.details.limits.iter().map(|&(label, used)| {
                    h_flex()
                        .h(px(26.))
                        .px_2p5()
                        .gap_3()
                        .text_size(ui::META)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground.opacity(0.8))
                        .child(div().w(px(104.)).truncate().child(label))
                        .child(
                            div()
                                .flex_1()
                                .h(px(4.))
                                .rounded_full()
                                .bg(theme.border)
                                .child(
                                    div()
                                        .h_full()
                                        .rounded_full()
                                        .w(gpui::relative(used))
                                        .bg(usage_color(used, theme)),
                                ),
                        )
                        .child(
                            div()
                                .w(px(36.))
                                .flex()
                                .justify_end()
                                .child(format!("{}%", (used * 100.).round() as u32)),
                        )
                }))
                .into_any_element(),
        )
    }
}

/// The bar's fill: plain until the window is nearly spent.
fn usage_color(used: f32, theme: &Theme) -> Hsla {
    if used >= 0.9 {
        theme.danger
    } else if used >= 0.75 {
        theme.warning
    } else {
        theme.foreground.opacity(0.7)
    }
}
