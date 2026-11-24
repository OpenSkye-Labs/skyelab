//! Linked pull requests' state, and the shown thread's background fetch. A PR is read by its URL with its thread's
//! own account's `gh`, only while its folder is still bound to that account, and only while it can still change:
//! merged PRs are never read again, and a closed one only when its thread is opened or focused, hourly (it may be
//! reopened).

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
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// An open PR is read again after this long.
pub(super) const POLL_EVERY_MS: i64 = 5 * 60_000;
/// The shown thread's PR is read again on window focus or when it is opened, after this long.
pub(super) const SHOWN_FRESH_MS: i64 = 60_000;
/// A closed (not merged) PR is read again when its thread is opened or focused, at most this often.
pub(super) const CLOSED_RECHECK_MS: i64 = 60 * 60_000;
const TICK: Duration = Duration::from_secs(30);
const HUNG_MS: i64 = 10 * 60_000;

/// What the details card and the sidebar show for a thread's PR.
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

/// When each thread's PR read last landed, for its link, shared by every window's reads. A read is taken where it
/// lands, off the UI thread, under this lock: it is dropped when another landed after it was asked, else its state
/// is stored before the lock is let go. The UI keeps exactly the reads taken here, so the stored state and the shown
/// one keep the same newest read.
#[derive(Clone, Default)]
pub(crate) struct PrLanded(Arc<Mutex<HashMap<i64, (String, i64)>>>);

impl Global for PrLanded {}

impl PrLanded {
    pub fn get(cx: &mut gpui::App) -> Self {
        cx.default_global::<Self>().clone()
    }

    /// Records that a read of `thread`'s PR at `url` landed `at` (tests stage a newer landing with it).
    #[cfg(test)]
    pub fn mark(&self, thread: i64, url: &str, at: i64) {
        self.lock().insert(thread, (url.to_owned(), at));
    }

    /// Takes a read of `thread`'s PR at `url` asked at `asked`, unless another landed after that: `store` runs first,
    /// under the lock, so taken reads are stored in the order they land. The landing time, or `None` when dropped.
    fn land(&self, thread: i64, url: &str, asked: i64, store: impl FnOnce()) -> Option<i64> {
        let mut landed = self.lock();
        if landed
            .get(&thread)
            .is_some_and(|(u, at)| u == url && *at > asked)
        {
            return None;
        }
        store();
        let at = crate::ui::now_ms();
        landed.insert(thread, (url.to_owned(), at));
        Some(at)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<i64, (String, i64)>> {
        // Only plain inserts happen under it, so a panic elsewhere leaves the map whole.
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// One batch of reads at a time, each read after the last.
#[derive(Default)]
pub(super) struct Watch {
    /// When the running batch started (ms). A batch this old is taken as hung (gh has no timeout) and a new one runs.
    polling_since: Option<i64>,
    _tick: Option<Task<()>>,
}

/// Whether to read the PR at `url` now, by its state as last read (or as the store kept it across a restart): never
/// once merged; once closed, only for the `shown` thread (opened or focused) and only [`CLOSED_RECHECK_MS`] after
/// its last read, since it may have been reopened; else when its last read for this link is `max_age` old.
pub(super) fn due(
    stored: Option<&str>,
    entry: Option<&PrEntry>,
    url: &str,
    now: i64,
    max_age: i64,
    shown: bool,
) -> bool {
    let entry = entry.filter(|e| e.url == url);
    let last = match entry.map(|e| &e.view) {
        Some(PrView::Status(s)) => Some(s.state),
        _ => stored.and_then(PrState::parse),
    };
    let max_age = match last {
        Some(PrState::Merged) => return false,
        Some(PrState::Closed) if !shown => return false,
        Some(PrState::Closed) => CLOSED_RECHECK_MS,
        _ => max_age,
    };
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
                let fresh = shown_fresh && Some(t.id) == shown;
                let max_age = if fresh { SHOWN_FRESH_MS } else { POLL_EVERY_MS };
                let entry = statuses.and_then(|s| s.0.get(&t.id));
                due(t.pr_state.as_deref(), entry, &url, now, max_age, fresh).then(|| Job {
                    thread: t.id,
                    url,
                    dir: t.cwd.clone(),
                    account: t.account_id.clone(),
                })
            })
            .collect();
        if jobs.is_empty() {
            return;
        }
        self.pr_watch.polling_since = Some(now);
        self.read_prs(jobs, Some(now), cx);
    }

    /// Reads `thread`'s PR now, whatever its last read (an action on it just ran), beside any running batch.
    pub(super) fn reread_pr(&mut self, thread: i64, cx: &mut Context<Self>) {
        let Some(job) = self.threads.iter().find(|t| t.id == thread).and_then(|t| {
            Some(Job {
                thread: t.id,
                url: t.pr_url.clone()?,
                dir: t.cwd.clone(),
                account: t.account_id.clone(),
            })
        }) else {
            return;
        };
        self.read_prs(vec![job], None, cx);
    }

    /// Reads `jobs` one after another off the UI thread; `started` is the batch mark it clears when done.
    fn read_prs(&mut self, jobs: Vec<Job>, started: Option<i64>, cx: &mut Context<Self>) {
        // A read asked before another one landed is older news: dropped, from the store and the UI alike.
        let gh = super::gh_bin(cx);
        let accounts_file = cx.try_global::<super::AccountsFile>().map(|f| f.0.clone());
        let (config, store, landed) = (
            self.config.clone(),
            self.backend.store.clone(),
            PrLanded::get(cx),
        );
        let read = cx.background_spawn(async move {
            let config = super::accounts_now(accounts_file.as_deref(), &config);
            jobs.into_iter()
                .map(|j| {
                    let asked = crate::ui::now_ms(); // per read: a batch's later reads start later
                    let view = read_pr(&config, &j.dir, &j.account, &j.url, &gh);
                    let taken = landed.land(j.thread, &j.url, asked, || {
                        // Stored, so an ended PR isn't read again after a restart.
                        if let PrView::Status(s) = &view
                            && let Err(e) = super::lock(&store).and_then(|st| {
                                st.set_thread_pr_state(j.thread, &j.url, s.state.as_str())
                            })
                        {
                            eprintln!("skyelab: saving PR state failed: {e:#}");
                        }
                    });
                    (j.thread, j.url, view, taken)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let read = read.await;
            let _ = this.update(cx, |this, cx| {
                // A batch taken as hung may end after a newer one started: that one's mark stays.
                if started.is_some() && this.pr_watch.polling_since == started {
                    this.pr_watch.polling_since = None;
                }
                let statuses = cx.default_global::<PrStatuses>();
                let mut changed = false;
                let mut states = Vec::new();
                for (thread, url, view, landed) in read {
                    // Dropped, or a batch that ends late bringing a read older than one shown since.
                    let Some(landed) = landed.filter(|at| {
                        statuses
                            .0
                            .get(&thread)
                            .is_none_or(|o| o.url != url || o.checked_ms <= *at)
                    }) else {
                        continue;
                    };
                    if let PrView::Status(s) = &view {
                        states.push((thread, url.clone(), s.state));
                    }
                    let old = statuses.0.insert(
                        thread,
                        PrEntry {
                            url: url.clone(),
                            view: view.clone(),
                            checked_ms: landed,
                        },
                    );
                    changed |= old.is_none_or(|o| o.url != url || o.view != view);
                }
                // Most reads find nothing new: redraw only for a change, in every window (they share the statuses).
                if changed {
                    this.pr_states_read(&states, cx);
                    this.each_other_window(cx, move |ws, cx| ws.pr_states_read(&states, cx));
                }
            });
        })
        .detach(); // `this` is weak; a hung gh only holds its own thread
    }

    /// New PR reads landed: the rows keep their states, and the sidebar and Details redraw.
    fn pr_states_read(&mut self, states: &[(i64, String, PrState)], cx: &mut Context<Self>) {
        for (thread, url, state) in states {
            if let Some(row) = self.threads.iter_mut().find(|t| t.id == *thread)
                && row.pr_url.as_deref() == Some(url.as_str())
            {
                row.pr_state = Some(state.as_str().to_owned());
            }
        }
        self.refresh_sidebar(cx);
        self.details.update(cx, |_, cx| cx.notify());
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

/// The review decision's glyph and tone, `None` when there is none.
pub(crate) fn review_look(
    review: skyelab_git::Review,
    theme: &Theme,
) -> Option<(IconName, Hsla, &'static str)> {
    use skyelab_git::Review;
    match review {
        Review::None => None,
        Review::Approved => Some((
            IconName::UserCheck,
            ui::ink(theme, theme.success),
            "Approved",
        )),
        Review::ChangesRequested => Some((
            IconName::UserRoundX,
            ui::ink(theme, theme.warning),
            "Changes requested",
        )),
        Review::Required => Some((IconName::User, theme.muted_foreground, "Awaiting review")),
    }
}

#[cfg(test)]
#[path = "pr_watch_test.rs"]
mod tests;
