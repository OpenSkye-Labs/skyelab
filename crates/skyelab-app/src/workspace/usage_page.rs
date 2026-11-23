//! Settings → Usage (T3's Usage page): every account side by side, from what this machine stored. Each account's
//! newest limit readings, and what its turns cost and used today, in 7 and in 30 days, per account and per project,
//! with a daily chart. Nothing asks an account: the numbers are the agents' own reports (Result and RateLimit events).

use super::details_panel::{AccountUsage, account_usage};
use gpui::{App, AppContext as _, Context, FocusHandle, Focusable, Task};
use skyelab_core::AccountsConfig;
use skyelab_store::usage::{self, Range, Spend, UsageDay};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// What the page shows its bars and chart in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Metric {
    #[default]
    Cost,
    Tokens,
}

impl Metric {
    pub(super) fn of(self, s: &Spend) -> f64 {
        match self {
            Metric::Cost => s.cost_usd,
            Metric::Tokens => s.tokens() as f64,
        }
    }
}

/// One read of the store: limits per account, and the window's turns grouped per account, project and day.
#[derive(Debug, Clone, Default)]
pub struct UsageData {
    pub accounts: Vec<AccountUsage>,
    pub rows: Vec<UsageDay>,
    pub projects: HashMap<i64, String>,
}

/// One account's line on the page, for the chosen range.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountLine {
    pub usage: AccountUsage,
    pub spend: Spend,
    /// Its part of every account's total in the chosen metric, 0 to 1.
    pub share: f64,
}

/// One project's spend under one account, for the chosen range.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectLine {
    pub project: String,
    pub account_id: String,
    pub spend: Spend,
}

/// What render draws: derived from [`UsageData`] when it loads or the range or metric changes, never per frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageView {
    pub accounts: Vec<AccountLine>,
    /// Per day of the window (today last): each account's value in the chosen metric, in `accounts` order.
    pub days: Vec<Vec<f64>>,
    /// The tallest day's total, for the chart's scale.
    pub peak: f64,
    pub projects: Vec<ProjectLine>,
    pub total: Spend,
}

/// Builds the page's lines: accounts in accounts.toml order (each one, even with no turns), projects by the chosen
/// metric, highest first.
pub fn derive(data: &UsageData, range: Range, metric: Metric) -> UsageView {
    let total = usage::spend(&data.rows, range, |_| true);
    let accounts: Vec<AccountLine> = data
        .accounts
        .iter()
        .map(|a| {
            let spend = usage::spend(&data.rows, range, |r| r.account_id == a.id);
            let all = metric.of(&total);
            AccountLine {
                usage: a.clone(),
                share: if all > 0.0 {
                    metric.of(&spend) / all
                } else {
                    0.0
                },
                spend,
            }
        })
        .collect();
    let per_account: Vec<Vec<Spend>> = accounts
        .iter()
        .map(|a| usage::daily(&data.rows, |r| r.account_id == a.usage.id))
        .collect();
    let days: Vec<Vec<f64>> = (0..usage::DAYS as usize)
        .map(|d| per_account.iter().map(|a| metric.of(&a[d])).collect())
        .collect();
    let peak = days
        .iter()
        .map(|d| d.iter().sum::<f64>())
        .fold(0.0, f64::max);
    let mut keys: Vec<(i64, &str)> = data
        .rows
        .iter()
        .map(|r| (r.project_id, r.account_id.as_str()))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    let mut projects: Vec<ProjectLine> = keys
        .into_iter()
        .map(|(p, a)| ProjectLine {
            project: data
                .projects
                .get(&p)
                .cloned()
                .unwrap_or_else(|| "Removed project".into()),
            account_id: a.to_owned(),
            spend: usage::spend(&data.rows, range, |r| {
                r.project_id == p && r.account_id == a
            }),
        })
        .filter(|l| l.spend.turns > 0)
        .collect();
    projects.sort_by(|a, b| metric.of(&b.spend).total_cmp(&metric.of(&a.spend)));
    UsageView {
        accounts,
        days,
        peak,
        projects,
        total,
    }
}

/// Local midnight today, in ms since the epoch.
fn today_start() -> i64 {
    use chrono::{Local, TimeZone as _};
    let midnight = Local::now().date_naive().and_hms_opt(0, 0, 0);
    // A DST gap at midnight has no local midnight: UTC's is a day boundary too.
    midnight.map_or_else(super::super::ui::now_ms, |m| {
        Local
            .from_local_datetime(&m)
            .earliest()
            .map_or_else(|| m.and_utc().timestamp_millis(), |t| t.timestamp_millis())
    })
}

pub struct UsagePage {
    focus: FocusHandle,
    pub range: Range,
    pub metric: Metric,
    /// `None` until the first read lands.
    pub data: Option<UsageData>,
    pub view: UsageView,
    /// The read in flight; a newer one replaces it.
    _load: Option<Task<()>>,
}

impl UsagePage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            range: Range::default(),
            metric: Metric::default(),
            data: None,
            view: UsageView::default(),
            _load: None,
        }
    }

    /// Reads every account's limits and the last 30 days of turns off the UI thread.
    pub fn reload(
        &mut self,
        store: Arc<Mutex<skyelab_store::Store>>,
        config: &AccountsConfig,
        cx: &mut Context<Self>,
    ) {
        let accounts = config.accounts.clone();
        let since = usage::since(today_start());
        let read = cx.background_spawn(async move {
            let s = super::lock(&store)?;
            let projects = s
                .list_projects()?
                .into_iter()
                .map(|p| (p.id, p.name))
                .collect();
            anyhow::Ok(UsageData {
                accounts: accounts.iter().map(|a| account_usage(&s, a)).collect(),
                rows: s.usage_days(since)?,
                projects,
            })
        });
        self._load = Some(cx.spawn(async move |this, cx| match read.await {
            Ok(data) => {
                let _ = this.update(cx, |this, cx| {
                    this.data = Some(data);
                    this.rederive(cx);
                });
            }
            Err(e) => eprintln!("reading usage failed: {e:#}"),
        }));
    }

    pub(super) fn set_range(&mut self, range: Range, cx: &mut Context<Self>) {
        self.range = range;
        self.rederive(cx);
    }

    pub(super) fn set_metric(&mut self, metric: Metric, cx: &mut Context<Self>) {
        self.metric = metric;
        self.rederive(cx);
    }

    fn rederive(&mut self, cx: &mut Context<Self>) {
        self.view = self
            .data
            .as_ref()
            .map(|d| derive(d, self.range, self.metric))
            .unwrap_or_default();
        cx.notify();
    }
}

impl Focusable for UsagePage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

#[cfg(all(test, unix))] // the fake agents are shell scripts
#[path = "usage_page_test.rs"]
mod usage_page_test;
