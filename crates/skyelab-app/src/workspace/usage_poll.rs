//! Asks each account's own CLI for its usage limits while Settings → Usage or the details card is shown
//! (`skyelab_agent::usage_limits`; docs/decisions/agents.md, "Usage limits are asked of the account's CLI"). The
//! readings go into the store like a turn's, so the page and the card read them the way they always have; what agents
//! report during turns stays the other source. Nothing is asked while neither is shown.

use super::Workspace;
use gpui::{AppContext as _, Context, Task};
use skyelab_agent::Limits;
use skyelab_core::Provider;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How often a shown account is asked, like T3's provider refresh.
const EVERY: Duration = Duration::from_secs(5 * 60);
/// The longest wait: after repeated failures, and for an account with no limits to read (signed out, an API key).
const MAX: Duration = Duration::from_secs(60 * 60);
/// How often the poller looks at what is shown and which account is due.
const TICK: Duration = Duration::from_secs(30);

#[derive(Default)]
struct AccountPoll {
    next: Option<Instant>,
    failures: u32,
    in_flight: bool,
    error: Option<String>,
}

// shortcut: each window's workspace polls on its own, so two windows showing usage ask twice as often; make it a
// Global like LoginStatus if that matters.
#[derive(Default)]
pub(super) struct UsagePoll {
    accounts: HashMap<String, AccountPoll>,
    ticking: bool,
    _tick: Option<Task<()>>,
}

impl UsagePoll {
    /// The last poll's failure per account id, for the Usage page.
    pub(super) fn errors(&self) -> HashMap<String, String> {
        self.accounts
            .iter()
            .filter_map(|(id, p)| Some((id.clone(), p.error.clone()?)))
            .collect()
    }
}

/// The wait after `failures` failures in a row: doubling from [`EVERY`], never past [`MAX`].
fn backoff(failures: u32) -> Duration {
    EVERY.saturating_mul(1 << failures.min(8)).min(MAX)
}

impl Workspace {
    fn usage_shown(&self, cx: &gpui::App) -> bool {
        // A remote's accounts live on its machine; only this machine's CLIs are asked.
        !self.is_remote()
            && (self.details_open
                || (self.settings_open
                    && self
                        .settings
                        .as_ref()
                        .is_some_and(|s| s.read(cx).section == crate::settings::Section::Usage)))
    }

    /// Starts polling if usage is shown and nothing polls yet; it stops by itself once neither is shown.
    pub(super) fn poll_usage(&mut self, cx: &mut Context<Self>) {
        if self.usage_poll.ticking || !self.usage_shown(cx) {
            return;
        }
        self.usage_poll.ticking = true;
        self.usage_poll._tick = Some(cx.spawn(async move |this, cx| {
            while this
                .update(cx, |ws, cx| ws.poll_usage_tick(cx))
                .unwrap_or(false)
            {
                cx.background_executor().timer(TICK).await;
            }
        }));
    }

    /// Asks every due account once; `false` (and polling stops) when usage isn't shown any more.
    fn poll_usage_tick(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.usage_shown(cx) {
            self.usage_poll.ticking = false;
            return false;
        }
        let bins = self.login_bins(cx);
        let now = Instant::now();
        for account in self.config.accounts.clone() {
            let poll = self
                .usage_poll
                .accounts
                .entry(account.id.clone())
                .or_default();
            if poll.in_flight || poll.next.is_some_and(|n| n > now) {
                continue;
            }
            poll.in_flight = true;
            let bin = match account.provider {
                Provider::Claude => bins.claude.clone(),
                Provider::Codex => bins.codex.clone(),
                Provider::Cursor => bins.cursor.clone(),
            };
            let store = self.backend.store.clone();
            let id = account.id.clone();
            let read = cx.background_spawn(async move {
                let Limits::Readings(events) = skyelab_agent::usage_limits(&bin, &account)? else {
                    return anyhow::Ok(false);
                };
                let payloads = events
                    .iter()
                    .map(serde_json::to_string)
                    .collect::<Result<Vec<_>, _>>()?;
                super::lock(&store)?.record_usage(&account.id, &payloads, crate::ui::now_ms())?;
                Ok(true)
            });
            cx.spawn(async move |this, cx| {
                let got = read.await;
                let _ = this.update(cx, |ws, cx| ws.usage_polled(&id, got, cx));
            })
            .detach();
        }
        true
    }

    fn usage_polled(&mut self, id: &str, got: anyhow::Result<bool>, cx: &mut Context<Self>) {
        let Some(poll) = self.usage_poll.accounts.get_mut(id) else {
            return;
        };
        poll.in_flight = false;
        let wait = match got {
            Ok(read) => {
                (poll.failures, poll.error) = (0, None);
                if read { EVERY } else { MAX }
            }
            Err(e) => {
                poll.failures += 1;
                poll.error = Some(format!("{e:#}"));
                backoff(poll.failures)
            }
        };
        poll.next = Some(Instant::now() + wait);
        self.refresh_usage(cx);
        self.reload_usage(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{EVERY, MAX, backoff};

    #[test]
    fn failures_back_off_to_an_hour() {
        assert_eq!(backoff(1), EVERY * 2);
        assert_eq!(backoff(2), EVERY * 4);
        assert_eq!(backoff(4), MAX);
        assert_eq!(backoff(40), MAX, "no overflow");
    }
}
