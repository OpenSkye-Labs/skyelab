//! Linked pull requests' state, and the shown thread's background fetch. A PR is read by its URL with its thread's
//! own account's `gh`, only while its folder is still bound to that account, and only while it can still change:
//! merged and closed PRs are never read again.

use super::Workspace;
use super::worktree::resolves_to;
use crate::ui;
use gpui::{AppContext as _, Context, Global, Hsla, SharedString, Task};
use gpui_component::Theme;
use gpui_kit_assets::IconName;
use skyelab_core::AccountsConfig;
use skyelab_git::{Checks, GitHub, PrState, PrStatus};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// An open PR is read again after this long.
pub(super) const POLL_EVERY_MS: i64 = 5 * 60_000;
/// The shown thread's PR is read again on window focus or when it is opened, after this long.
pub(super) const SHOWN_FRESH_MS: i64 = 60_000;
const TICK: Duration = Duration::from_secs(30);
const HUNG_MS: i64 = 10 * 60_000;

/// What the Details tab and the sidebar show for a thread's PR.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PrView {
    Status(PrStatus),
    /// The account has no GitHub login: gh was never run.
    SignIn,
    Failed(SharedString),
}

pub(crate) struct PrEntry {
    /// The link this was read for: a relinked thread starts over.
    pub url: String,
    pub view: PrView,
    pub checked_ms: i64,
}

/// Every read PR, by thread id.
#[derive(Default)]
pub(crate) struct PrStatuses(pub HashMap<i64, PrEntry>);

impl Global for PrStatuses {}

impl PrStatuses {
    /// The thread's PR as last read, when it was read for `url`.
    pub fn view<'a>(cx: &'a gpui::App, thread: i64, url: &str) -> Option<&'a PrView> {
        cx.try_global::<Self>()
            .and_then(|s| s.0.get(&thread))
            .filter(|e| e.url == url)
            .map(|e| &e.view)
    }
}

/// One batch of reads at a time, each read after the last.
#[derive(Default)]
pub(super) struct Watch {
    /// When the running batch started (ms). A batch this old is taken as hung (gh has no timeout) and a new one runs.
    polling_since: Option<i64>,
    _tick: Option<Task<()>>,
}

/// Whether to read the PR at `url` now: never once it is merged or closed (as last read, or as the store kept it
/// across a restart), else when its last read for this link is `max_age` old.
pub(super) fn due(
    stored: Option<&str>,
    entry: Option<&PrEntry>,
    url: &str,
    now: i64,
    max_age: i64,
) -> bool {
    let entry = entry.filter(|e| e.url == url);
    let last = match entry.map(|e| &e.view) {
        Some(PrView::Status(s)) => Some(s.state),
        _ => stored.and_then(PrState::parse),
    };
    if last.is_some_and(PrState::is_done) {
        return false;
    }
    entry.is_none_or(|e| now - e.checked_ms >= max_age)
}

/// Reads one PR, blocking: Push's account checks against `config` (accounts.toml as saved now), then the account's
/// gh, but only when that account has a GitHub login.
pub(super) fn read_pr(
    config: &AccountsConfig,
    dir: &Path,
    account: &str,
    url: &str,
    gh: &Path,
) -> PrView {
    // Not Push's repo check: the PR is read by URL, and a discarded worktree's folder is gone.
    if let Err(e) = resolves_to(config, dir, account) {
        return PrView::Failed(format!("{e:#}").into());
    }
    if config
        .resolve(dir)
        .is_some_and(|a| a.gh_config_dir.is_none())
    {
        return PrView::SignIn;
    }
    let gh_dir = match config.github_config_dir(dir) {
        Ok(d) => d,
        Err(e) => return PrView::Failed(format!("{e:#}").into()),
    };
    if !skyelab_git::gh_logged_in(&gh_dir) {
        return PrView::SignIn;
    }
    match skyelab_git::pr_status(
        &GitHub {
            gh,
            config_dir: &gh_dir,
        },
        url,
    ) {
        Ok(s) => PrView::Status(s),
        Err(e) => PrView::Failed(format!("{e:#}").into()),
    }
}

struct Job {
    thread: i64,
    url: String,
    dir: PathBuf,
    account: String,
    stored: Option<String>,
}

impl Workspace {
    /// Starts the timer that reads due PRs and fetches the shown thread's folder.
    pub(super) fn watch_remote(&mut self, cx: &mut Context<Self>) {
        self.pr_watch._tick = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                if this
                    .update(cx, |this, cx| {
                        // One window polls in the background; each still polls when it is focused.
                        if this.is_hub(cx) {
                            this.poll_prs(false, cx);
                        }
                        this.fetch_shown(cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    /// Reads every due PR of the listed threads, one after another off the UI thread. `shown_fresh` (focus, a thread
    /// opened) reads the shown thread's after [`SHOWN_FRESH_MS`] instead of [`POLL_EVERY_MS`].
    pub(super) fn poll_prs(&mut self, shown_fresh: bool, cx: &mut Context<Self>) {
        let now = crate::ui::now_ms();
        if self
            .pr_watch
            .polling_since
            .is_some_and(|t| now - t < HUNG_MS)
        {
            return;
        }
        let shown = self.shown_thread(cx);
        let statuses = cx.try_global::<PrStatuses>();
        let jobs: Vec<Job> = self
            .threads
            .iter()
            .filter_map(|t| {
                let url = t.pr_url.clone()?;
                let max_age = if shown_fresh && Some(t.id) == shown {
                    SHOWN_FRESH_MS
                } else {
                    POLL_EVERY_MS
                };
                let entry = statuses.and_then(|s| s.0.get(&t.id));
                due(t.pr_state.as_deref(), entry, &url, now, max_age).then(|| Job {
                    thread: t.id,
                    url,
                    dir: t.cwd.clone(),
                    account: t.account_id.clone(),
                    stored: t.pr_state.clone(),
                })
            })
            .collect();
        if jobs.is_empty() {
            return;
        }
        self.pr_watch.polling_since = Some(now);
        let started = now;
        let gh = super::gh_bin(cx);
        let accounts_file = cx.try_global::<super::AccountsFile>().map(|f| f.0.clone());
        let (config, store) = (self.config.clone(), self.backend.store.clone());
        let read = cx.background_spawn(async move {
            let config = super::accounts_now(accounts_file.as_deref(), &config);
            jobs.into_iter()
                .map(|j| {
                    let view = read_pr(&config, &j.dir, &j.account, &j.url, &gh);
                    // Stored, so an ended PR isn't read again after a restart.
                    if let PrView::Status(s) = &view
                        && j.stored.as_deref() != Some(s.state.as_str())
                        && let Err(e) = super::lock(&store).and_then(|st| {
                            st.set_thread_pr_state(j.thread, &j.url, s.state.as_str())
                        })
                    {
                        eprintln!("skyelab: saving PR state failed: {e:#}");
                    }
                    (j.thread, j.url, view)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let read = read.await;
            let _ = this.update(cx, |this, cx| {
                // A batch taken as hung may end after a newer one started: that one's mark stays.
                if this.pr_watch.polling_since == Some(started) {
                    this.pr_watch.polling_since = None;
                }
                let now = crate::ui::now_ms();
                let statuses = cx.default_global::<PrStatuses>();
                let mut changed = false;
                for (thread, url, view) in read {
                    if let PrView::Status(s) = &view
                        && let Some(row) = this.threads.iter_mut().find(|t| t.id == thread)
                        && row.pr_url.as_deref() == Some(url.as_str())
                    {
                        row.pr_state = Some(s.state.as_str().to_owned());
                    }
                    let old = statuses.0.insert(
                        thread,
                        PrEntry {
                            url: url.clone(),
                            view: view.clone(),
                            checked_ms: now,
                        },
                    );
                    changed |= old.is_none_or(|o| o.url != url || o.view != view);
                }
                // Most reads find nothing new: redraw only for a change.
                if changed {
                    this.refresh_sidebar(cx);
                    this.details.update(cx, |_, cx| cx.notify());
                }
            });
        })
        .detach(); // `this` is weak; a hung gh only holds its own thread
    }

    /// Fetches the shown thread's folder when the setting's interval has passed since its last fetch.
    pub(super) fn fetch_shown(&mut self, cx: &mut Context<Self>) {
        let minutes = super::thread_extras::prefs::get(cx).fetch_minutes;
        if minutes == 0 {
            return;
        }
        if let Some(view) = self.current_thread() {
            view.update(cx, |v, cx| v.fetch_if_due(i64::from(minutes) * 60_000, cx));
        }
    }
}

/// A sidebar row's PR badge: "#24", its state and its checks once read.
#[derive(Clone)]
pub(crate) struct PrBadge {
    pub label: SharedString,
    pub state: Option<PrState>,
    pub checks: Checks,
}

/// The row's PR badge: its number, and what the last read said (else the store's state).
pub(super) fn badge(t: &skyelab_store::Thread, url: &str, cx: &gpui::App) -> PrBadge {
    let (state, checks) = match PrStatuses::view(cx, t.id, url) {
        Some(PrView::Status(s)) => (Some(s.state), s.checks),
        _ => (
            t.pr_state.as_deref().and_then(skyelab_git::PrState::parse),
            skyelab_git::Checks::None,
        ),
    };
    PrBadge {
        label: super::thread_list::pr_label(url).into(),
        state,
        checks,
    }
}

/// T3's PR state glyphs and tones: open green, draft muted, merged violet, closed red; unread muted.
pub(crate) fn state_look(state: Option<PrState>, theme: &Theme) -> (IconName, Hsla, &'static str) {
    match state {
        None => (IconName::GitPullRequest, theme.muted_foreground, ""),
        Some(PrState::Open) => (
            IconName::GitPullRequest,
            ui::ink(theme, theme.success),
            "Open",
        ),
        Some(PrState::Draft) => (
            IconName::GitPullRequestDraft,
            theme.muted_foreground,
            "Draft",
        ),
        Some(PrState::Merged) => (IconName::GitMerge, ui::ink(theme, theme.magenta), "Merged"),
        Some(PrState::Closed) => (
            IconName::GitPullRequestClosed,
            ui::ink(theme, theme.danger),
            "Closed",
        ),
    }
}

/// The checks' glyph and tone, `None` when the PR has no checks.
pub(crate) fn checks_look(checks: Checks, theme: &Theme) -> Option<(IconName, Hsla, &'static str)> {
    match checks {
        Checks::None => None,
        Checks::Passing => Some((
            IconName::CircleCheck,
            ui::ink(theme, theme.success),
            "Checks passing",
        )),
        Checks::Pending => Some((
            IconName::CircleDot,
            ui::ink(theme, theme.warning),
            "Checks running",
        )),
        Checks::Failing => Some((
            IconName::CircleX,
            ui::ink(theme, theme.danger),
            "Checks failing",
        )),
    }
}

#[cfg(test)]
#[path = "pr_watch_test.rs"]
mod tests;
