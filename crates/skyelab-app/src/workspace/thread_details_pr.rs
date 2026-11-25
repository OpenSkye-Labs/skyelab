//! The details card's Pull request section (the link, its state, checks and review, after T3's
//! ThreadStatusIndicators) and the branch's upstream: the background fetch and its ahead/behind counts.

use super::super::OpenSettingsSection;
use super::super::pr_watch::{PrStatuses, PrView, checks_look, state_look};
use super::super::thread::{GitAction, ThreadView, ThreadViewEvent};
use super::On;
use super::card::{chevron, row, tinted_row};
use crate::ui;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use gpui::{AnyElement, App, AppContext as _, Context, SharedString, div, prelude::*};
use gpui_component::{ActiveTheme as _, v_flex};
use skyelab_git::Checks;

impl ThreadView {
    /// The thread's pull request (opened here or linked) with what its last read said, else "Link pull request…".
    /// `None` before the thread has a row to keep the link on.
    pub(super) fn render_pr(&self, on: &On, cx: &App) -> Option<AnyElement> {
        let theme = cx.theme();
        let thread = self.thread.as_ref()?;
        let Some(url) = thread.pr_url.clone() else {
            return Some(
                row(
                    "details-link-pr",
                    SkyeIcon::Plus,
                    "Link pull request…",
                    theme,
                )
                .on_click(on.click(|this, window, cx| this.open_link_pr(window, cx)))
                .into_any_element(),
            );
        };
        let view = PrStatuses::view(cx, thread.id, &url);
        let state = match view {
            Some(PrView::Status(s)) => Some(s.state),
            _ => thread
                .pr_state
                .as_deref()
                .and_then(skyelab_git::PrState::parse),
        };
        let (icon, color, word) = state_look(state, theme);
        let label = url.trim_start_matches("https://").to_owned();
        let open = url.clone();
        let link = tinted_row("details-pr", icon, color, label, theme)
            .on_click(move |_, _, cx| {
                if super::super::thread_render::opens_in_browser(&open) {
                    cx.open_url(&open);
                }
            })
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(ui::META)
                    .text_color(color)
                    .child(word),
            )
            .when(state.is_none_or(|s| !s.is_done()), |r| {
                r.child(
                    Button::new("details-pr-actions")
                        .ghost()
                        .xsmall()
                        .icon(SkyeIcon::GitMerge)
                        .tooltip("Merge or review…")
                        .on_click(on.click(|this, window, cx| {
                            cx.stop_propagation(); // not the row's open-in-browser
                            this.open_pr_actions(window, cx);
                        })),
                )
            })
            .child(
                Button::new("details-unlink-pr")
                    .ghost()
                    .xsmall()
                    .icon(SkyeIcon::Unlink)
                    .tooltip("Unlink pull request")
                    .on_click(on.click(|this, _, cx| {
                        cx.stop_propagation(); // not the row's open-in-browser
                        this.unlink_pr(cx);
                    })),
            );
        let meta = |id: &'static str, text: SharedString, color: gpui::Hsla| {
            div()
                .id(id)
                .px_2p5()
                .py_1()
                .text_size(ui::META)
                .text_color(color)
                .line_clamp(2)
                .child(text)
                .into_any_element()
        };
        let mut rows = vec![link.into_any_element()];
        match view {
            None => {}
            Some(PrView::SignIn) => rows.push(
                row(
                    "details-pr-sign-in",
                    SkyeIcon::Github,
                    "Sign in to GitHub",
                    theme,
                )
                .child(chevron(SkyeIcon::ChevronRight, theme))
                .on_click(|_, window, cx| {
                    let section = crate::settings::Section::SourceControl;
                    window.dispatch_action(Box::new(OpenSettingsSection { section }), cx);
                })
                .into_any_element(),
            ),
            Some(PrView::Failed(e)) => rows.push(meta(
                "details-pr-error",
                format!("Couldn't read the pull request: {e}").into(),
                ui::ink(theme, theme.danger),
            )),
            Some(PrView::Status(s)) => {
                if let Some((icon, color, text)) = checks_look(s.checks, theme) {
                    let text = match s.checks {
                        Checks::Failing => format!("{} failing", plural(s.failed.len(), "check")),
                        _ => text.to_owned(),
                    };
                    rows.push(
                        tinted_row("details-pr-checks", icon, color, text, theme)
                            .cursor_default()
                            .into_any_element(),
                    );
                }
                for (i, f) in s.failed.iter().enumerate() {
                    let url = f.url.clone();
                    rows.push(
                        tinted_row(
                            SharedString::from(format!("details-pr-check-{i}")),
                            SkyeIcon::CircleX,
                            ui::ink(theme, theme.danger),
                            f.name.clone(),
                            theme,
                        )
                        .pl_6()
                        .when(url.is_some(), |r| {
                            r.child(chevron(SkyeIcon::ExternalLink, theme))
                        })
                        .when(url.is_none(), |r| r.cursor_default())
                        .on_click(move |_, _, cx| {
                            if let Some(url) = &url
                                && super::super::thread_render::opens_in_browser(url)
                            {
                                cx.open_url(url);
                            }
                        })
                        .into_any_element(),
                    );
                }
                let review = super::super::pr_watch::review_look(s.review, theme);
                rows.extend(review.map(|(icon, color, text)| {
                    tinted_row("details-pr-review", icon, color, text, theme)
                        .cursor_default()
                        .into_any_element()
                }));
            }
        }
        Some(v_flex().children(rows).into_any_element())
    }

    /// The merge and review dialog for the thread's PR, acting with the thread's account in its folder; the PR is
    /// read again after an action.
    pub(in crate::workspace) fn open_pr_actions(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Entity<super::super::pr_actions::PrActions>> {
        let thread = self.thread.as_ref()?;
        let url = thread.pr_url.clone()?;
        let (checks, head) = match PrStatuses::view(cx, thread.id, &url) {
            Some(PrView::Status(s)) => (Some(s.checks), s.head.clone()),
            _ => (None, None),
        };
        let target = super::super::pr_actions::Target {
            number: url
                .rsplit('/')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0),
            url,
            checks,
            head,
            dir: self.cwd.clone(),
            account: thread.account_id.clone(),
            config: self.config.clone(),
        };
        let view = cx.weak_entity();
        Some(super::super::pr_actions::open(
            target,
            move |_, _, cx| _ = view.update(cx, |_, cx| cx.emit(ThreadViewEvent::PrActed)),
            window,
            cx,
        ))
    }

    /// "2 behind / 1 ahead" of the branch's upstream, for the branch row; nothing when even or without one.
    pub(super) fn sync_label(&self) -> Option<String> {
        let (ahead, behind) = self.details.sync?;
        let parts: Vec<String> = [(behind, "behind"), (ahead, "ahead")]
            .into_iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, w)| format!("{n} {w}"))
            .collect();
        (!parts.is_empty()).then(|| parts.join(" / "))
    }

    /// "Pull N commits" under the branch when the upstream is ahead, and why the last fetch failed.
    pub(super) fn render_sync_rows(&self, on: &On, cx: &App) -> Vec<AnyElement> {
        let theme = cx.theme();
        let behind = self.details.sync.map_or(0, |(_, behind)| behind);
        let pull = (behind > 0).then(|| {
            row(
                "details-pull",
                SkyeIcon::ArrowDown,
                format!("Pull {}", plural(behind as usize, "commit")),
                theme,
            )
            .on_click(on.click(|this, _, cx| {
                this.details_home(cx);
                cx.emit(ThreadViewEvent::Git(GitAction::Pull));
            }))
            .into_any_element()
        });
        let failed = self.details.fetch_error.clone().map(|e| {
            div()
                .id("details-fetch-error")
                .px_2p5()
                .py_1()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .line_clamp(2)
                .child(format!("Fetch failed: {e}"))
                .into_any_element()
        });
        pull.into_iter().chain(failed).collect()
    }

    /// Fetches the thread's folder off the UI thread when its last fetch is `every_ms` old, under Push's checks and
    /// the account's own gh credentials, then counts ahead/behind again.
    pub(in crate::workspace) fn fetch_if_due(&mut self, every_ms: i64, cx: &mut Context<Self>) {
        let now = ui::now_ms();
        let Some(account) = self.thread.as_ref().map(|t| t.account_id.clone()) else {
            return;
        };
        if self.details.fetching || now - self.details.fetched_ms < every_ms {
            return;
        }
        (self.details.fetching, self.details.fetched_ms) = (true, now);
        let (dir, config) = (self.cwd.clone(), self.config.clone());
        let accounts_file = cx
            .try_global::<super::super::AccountsFile>()
            .map(|f| f.0.clone());
        let gh = super::super::gh_bin(cx);
        let task = cx.background_spawn(async move {
            if skyelab_git::ahead_behind(&dir)?.is_none() {
                return Ok(None); // no upstream: nothing to fetch, so no login to ask for
            }
            let config = super::super::accounts_now(accounts_file.as_deref(), &config);
            super::super::worktree::push_account(&config, &dir, &account)?;
            let config_dir = config.github_config_dir(&dir)?;
            let github = skyelab_git::GitHub {
                gh: &gh,
                config_dir: &config_dir,
            };
            skyelab_git::fetch(&dir, &github)?;
            anyhow::Ok(skyelab_git::ahead_behind(&dir)?)
        });
        self.details._fetch = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.details.fetching = false;
                match result {
                    Ok(sync) => {
                        this.details.sync = sync;
                        this.details.fetch_error = None;
                    }
                    Err(e) => this.details.fetch_error = Some(format!("{e:#}").into()),
                }
                cx.notify();
            });
        }));
    }

    /// Forgets the thread's pull request (the PR itself is untouched); the workspace's notice can undo it.
    pub(in crate::workspace) fn unlink_pr(&mut self, cx: &mut Context<Self>) {
        let Some((id, Some(url))) = self.thread.as_ref().map(|t| (t.id, t.pr_url.clone())) else {
            return;
        };
        let store = self.backend.store.clone();
        let saved = cx.background_spawn(async move {
            super::super::lock(&store).and_then(|s| s.set_thread_pr_url(id, None))
        });
        cx.spawn(async move |this, cx| {
            let saved = saved.await;
            let _ = this.update(cx, |this, cx| match saved {
                Err(e) => {
                    this.error = Some(format!("Couldn't unlink the pull request: {e:#}").into());
                    cx.notify();
                }
                Ok(()) => {
                    this.set_pr_url(None, cx);
                    cx.emit(ThreadViewEvent::PrUnlinked(url));
                }
            });
        })
        .detach(); // one short write; `this` is weak
    }
}

/// "1 commit", "3 checks".
fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}
