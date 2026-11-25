//! The details card's version control rows, Lineage and Pull request sections.

use super::super::OpenThread;
use super::super::thread::{ChildLink, ThreadView};
use super::super::thread::{GitAction, ThreadViewEvent};
use super::super::thread_worktree::{DiscardWorktree, OpenChanges, OpenWorktreeTerminal};
use super::card::{chevron, row};
use super::{On, Page, is_running};
use crate::ui::Button;
use crate::ui::SkyeIcon;
use crate::ui::{self, Status, TextField};
use gpui::Focusable as _;
use gpui::{
    AnyElement, App, AppContext as _, Context, FontWeight, SharedString, Window, div, prelude::*,
};
use gpui_component::{
    ActiveTheme as _, h_flex,
    input::{InputEvent, InputState},
    v_flex,
};

impl ThreadView {
    pub(super) fn render_git_rows(&self, on: &On, cx: &App) -> Option<AnyElement> {
        let theme = cx.theme();
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
        let sync = self.sync_label().map(|s| {
            div()
                .id("details-sync")
                .flex_shrink_0()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(s)
        });
        let branch_row = match worktree_branch {
            // A worktree thread's branch is its own; it doesn't switch.
            Some(branch) => row("details-branch", SkyeIcon::GitBranch, branch, theme)
                .children(sync)
                .cursor_default(),
            None => {
                let picker = self.branch_picker.clone();
                row(
                    "details-branch",
                    SkyeIcon::GitBranch,
                    self.branch_picker.read(cx).label(picks_base),
                    theme,
                )
                .children(sync)
                .child(chevron(SkyeIcon::ChevronDown, theme))
                .debug_selector(|| "details-branch".into())
                .on_click(on.click(move |_, window, cx| {
                    picker.update(cx, |p, cx| p.open_focused(window, cx));
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
                        .text_color(ui::ink(theme, theme.success))
                        .child(format!("+{added}")),
                )
                .child(
                    div()
                        .text_color(ui::ink(theme, theme.danger))
                        .child(format!("−{removed}")),
                )
        });
        let has_worktree = thread.is_some_and(|t| t.worktree_path.is_some());
        Some(
            v_flex()
                .child(branch_row)
                .children(self.render_sync_rows(on, cx))
                .child(
                    row(
                        "details-commit",
                        SkyeIcon::CloudUpload,
                        "Commit & push",
                        theme,
                    )
                    .child(chevron(SkyeIcon::ChevronRight, theme))
                    .on_click(on.click(|this, _, cx| this.open_git_menu(cx))),
                )
                .child(
                    row(
                        "details-changes",
                        SkyeIcon::FileDiff,
                        format!("Changes vs {base}"),
                        theme,
                    )
                    .children(stat)
                    .on_click(on.click(|this, window, cx| {
                        this.details_home(cx);
                        this.open_changes(&OpenChanges, window, cx);
                    })),
                )
                .when(has_worktree, |v| {
                    v.child(
                        row(
                            "details-worktree-terminal",
                            SkyeIcon::SquareTerminal,
                            "Open terminal here",
                            theme,
                        )
                        .on_click(on.click(|this, window, cx| {
                            this.details_home(cx);
                            this.open_worktree_terminal(&OpenWorktreeTerminal, window, cx);
                        })),
                    )
                    .child(
                        row(
                            "details-discard-worktree",
                            SkyeIcon::Trash,
                            "Discard worktree",
                            theme,
                        )
                        .on_click(on.click(|this, window, cx| {
                            this.details_home(cx);
                            // The confirm shows above the composer, where the card would cover it.
                            crate::workspace::dispatch(
                                Box::new(super::super::layout::ToggleThreadDetails),
                                window,
                                cx,
                            );
                            this.discard_worktree(&DiscardWorktree, window, cx);
                        })),
                    )
                })
                .into_any_element(),
        )
    }

    /// The running children, then a row to the finished ones; titled with how many run.
    pub(super) fn render_lineage(&self, on: &On, cx: &App) -> Option<(SharedString, AnyElement)> {
        let source = self.composer.extras.forked_from.clone();
        if self.children.is_empty() && source.is_none() {
            return None;
        }
        let theme = cx.theme();
        // Where this thread came from: its fork or handoff source.
        let source = source.map(|(id, label)| {
            row("details-source", SkyeIcon::GitBranch, label, theme).on_click(on.click(
                move |this, window, cx| {
                    this.details_home(cx);
                    crate::workspace::dispatch(Box::new(OpenThread { id }), window, cx);
                },
            ))
        });
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
            .children(source)
            .children(running.into_iter().map(|c| self.agent_row(c, on, cx)))
            .when(previous > 0, |v| {
                v.child(
                    row("details-previous", SkyeIcon::Bot, "Previous agents", theme)
                        .child(
                            div()
                                .text_size(ui::META)
                                .text_color(theme.muted_foreground)
                                .child(previous.to_string()),
                        )
                        .child(chevron(SkyeIcon::ChevronRight, theme))
                        .on_click(on.click(|this, _, cx| this.details_page(Page::Previous, cx))),
                )
            });
        Some((title, body.into_any_element()))
    }

    /// The Commit & push menu, and who it pushes as (read from the account's `gh` now, off the UI thread).
    pub(in crate::workspace) fn open_git_menu(&mut self, cx: &mut Context<Self>) {
        self.details.page = Page::Git;
        self.details.gh_login = None;
        let (dir, config) = (self.cwd.clone(), self.config.clone());
        let account = self.thread.as_ref().map(|t| t.account_id.clone());
        let accounts_file = cx
            .try_global::<super::super::AccountsFile>()
            .map(|f| f.0.clone());
        let gh_bin = super::super::gh_bin(cx);
        let task = cx.background_spawn(async move {
            // Push's own checks, against accounts.toml as saved now: a rebind since the thread started refuses.
            let account = account.ok_or("send a prompt first: the thread has no account yet")?;
            let config = super::super::accounts_now(accounts_file.as_deref(), &config);
            let checked = super::super::worktree::push_account(&config, &dir, &account)
                .and_then(|()| config.github_config_dir(&dir));
            let dir = checked.map_err(|e| format!("{e:#}"))?;
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

    /// T3's GitActionsControl as a page: Commit, Commit & push, Push, Pull, then Open PR (View PR once there is one). Each
    /// runs in the Changes tab, where its message and result show.
    pub(super) fn render_git_menu(&self, on: &On, cx: &App) -> Vec<AnyElement> {
        let theme = cx.theme();
        let login = self
            .details
            .gh_login
            .clone()
            .unwrap_or_else(|| "Checking the GitHub login…".into());
        let action = |id: &'static str, icon: SkyeIcon, label: &'static str, a: GitAction| {
            row(id, icon, label, theme)
                .on_click(on.click(move |this, _, cx| {
                    this.details_home(cx);
                    cx.emit(ThreadViewEvent::Git(a));
                }))
                .into_any_element()
        };
        let pr = match self.thread.as_ref().and_then(|t| t.pr_url.clone()) {
            Some(url) => row(
                "details-git-view-pr",
                SkyeIcon::ExternalLink,
                "View PR",
                theme,
            )
            .on_click(move |_, _, cx| {
                if super::super::thread_render::opens_in_browser(&url) {
                    cx.open_url(&url);
                }
            })
            .into_any_element(),
            None => action(
                "details-git-open-pr",
                SkyeIcon::GitPullRequest,
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
                SkyeIcon::GitCommitVertical,
                "Commit",
                GitAction::Commit,
            ),
            action(
                "details-git-commit-push",
                SkyeIcon::CloudUpload,
                "Commit & push",
                GitAction::CommitPush,
            ),
            action(
                "details-git-push",
                SkyeIcon::ArrowUp,
                "Push",
                GitAction::Push,
            ),
            action(
                "details-git-pull",
                SkyeIcon::ArrowDown,
                "Pull",
                GitAction::Pull,
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
        let store = self.backend.store.clone();
        let link = url.clone();
        let saved = cx.background_spawn(async move {
            super::super::lock(&store).and_then(|s| s.set_thread_pr_url(id, Some(&link)))
        });
        let input = input.clone();
        cx.spawn_in(window, async move |this, cx| {
            let saved = saved.await;
            let _ = this.update_in(cx, |this, window, cx| match saved {
                Err(e) => {
                    this.details.pr_error = Some(format!("Couldn't save the link: {e:#}").into());
                    cx.notify();
                }
                Ok(()) => {
                    input.update(cx, |s, cx| s.set_value("", window, cx));
                    this.set_pr_url(Some(url), cx);
                    this.details_home(cx);
                }
            });
        })
        .detach(); // one short write; `this` is weak
    }

    pub(super) fn render_link_pr(&self, on: &On, cx: &App) -> Vec<AnyElement> {
        let theme = cx.theme();
        let Some(input) = &self.details.pr_input else {
            return Vec::new();
        };
        vec![
            h_flex()
                .px_2p5()
                .py_1()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(TextField::new(input).small()),
                )
                .child(
                    Button::new("details-link-pr-save")
                        .small()
                        .primary()
                        .label("Link")
                        .on_click(on.click(|this, window, cx| this.link_pr(window, cx))),
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
                .text_color(ui::ink(theme, theme.danger))
                .child(e)
                .into_any_element()
        }))
        .collect()
    }

    /// A delegated thread: its status dot, title, and how long it has run (or how it ended); opens it.
    pub(super) fn agent_row(&self, c: &ChildLink, on: &On, cx: &App) -> AnyElement {
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
            .on_click(on.click(move |this, window, cx| {
                this.details_home(cx);
                crate::workspace::dispatch(Box::new(OpenThread { id }), window, cx);
            }))
            .into_any_element()
    }
}
