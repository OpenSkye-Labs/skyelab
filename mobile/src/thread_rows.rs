//! The thread list, the desktop's flat list at phone width: Pinned, Working, then by the day each thread last
//! changed, plus collapsible Snoozed and Settled shelves (protocol 1.6), each row with its project, account, status
//! and preview. And the open thread's status line.

use std::collections::BTreeMap;

use skyelab_agent::Event;
use skyelab_agent::thread_list::{DAYS, day_bucket, preview_text};
use skyelab_agent::timeline::{Record, limit_after};
use skyelab_core::snooze;
use skyelab_protocol::{Project, Thread};

/// Current wall-clock time in Unix milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Whether a thread's agent is at work (the desktop's Starting or Running).
pub fn working(t: &Thread) -> bool {
    matches!(t.status.as_str(), "starting" | "running")
}

/// The active flat list sections plus the Snoozed and Settled shelves.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ThreadLists {
    pub active: Vec<(&'static str, Vec<Thread>)>,
    pub snoozed: Vec<Thread>,
    pub settled: Vec<Thread>,
}

/// Splits `threads` into active sections (Pinned, Working, `DAYS`), Snoozed (still asleep at `now_ms`, soonest
/// waking first), and Settled (most recently settled first). Matches desktop precedence: only working beats a
/// shelf, and a shelf beats pinned.
pub fn partition(mut threads: Vec<Thread>, starts: [i64; 3], now_ms: i64) -> ThreadLists {
    threads.sort_by_key(|t| std::cmp::Reverse(t.updated_at));
    let mut active: Vec<(&'static str, Vec<Thread>)> = ["Pinned", "Working"]
        .into_iter()
        .chain(DAYS)
        .map(|title| (title, Vec::new()))
        .collect();
    let mut snoozed = Vec::new();
    let mut settled = Vec::new();
    for t in threads {
        let busy = working(&t);
        if !busy && snooze::is_asleep(t.snoozed_until, now_ms) {
            snoozed.push(t);
        } else if !busy && t.settled_at.is_some() {
            settled.push(t);
        } else {
            let at = match (t.pinned, busy) {
                (true, _) => 0,
                (false, true) => 1,
                _ => 2 + day_bucket(starts, t.updated_at),
            };
            active[at].1.push(t);
        }
    }
    active.retain(|(_, ts)| !ts.is_empty());
    snoozed.sort_by_key(|t| t.snoozed_until);
    settled.sort_by_key(|t| std::cmp::Reverse(t.settled_at));
    ThreadLists {
        active,
        snoozed,
        settled,
    }
}

/// The active list's sections, empty ones left out: pinned, working, then `DAYS` by `updated_at` given where today,
/// yesterday and the 7-day window start (`skyelab_agent::thread_list::day_starts`). Newest first in each.
pub fn sections(threads: Vec<Thread>, starts: [i64; 3]) -> Vec<(&'static str, Vec<Thread>)> {
    partition(threads, starts, now_ms()).active
}

/// A short label for a snoozed thread: `"Wakes in 2h"` while asleep, `"Woke"` once its time passed, `None` else.
pub fn snooze_badge(t: &Thread, now_ms: i64) -> Option<String> {
    let until = t.snoozed_until?;
    if snooze::is_asleep(Some(until), now_ms) {
        Some(format!("Wakes in {}", snooze::wake_label(until, now_ms)))
    } else if snooze::is_woke(Some(until), now_ms) {
        Some("Woke".into())
    } else {
        None
    }
}

/// A row's second and third lines: "project · account · status", then the preview as one plain line.
pub fn row_detail(t: &Thread, project: Option<&Project>) -> String {
    let account = t.account_label.as_deref().unwrap_or(&t.account_id);
    let mut line: Vec<&str> = project.map(|p| p.name.as_str()).into_iter().collect();
    line.extend([account, t.status.as_str()]);
    let mut out = line.join(" · ");
    if let Some(preview) = t
        .preview
        .as_deref()
        .map(preview_text)
        .filter(|p| !p.is_empty())
    {
        out.push('\n');
        out.push_str(&preview);
    }
    out
}

/// What a thread's own events say about its account's usage: limited (until when, Unix seconds) and the latest
/// reading of each window.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    pub limit: Option<Option<i64>>,
    pub windows: BTreeMap<String, f32>,
}

impl Usage {
    pub fn observe(&mut self, record: &Record) {
        self.limit = limit_after(self.limit, record);
        if let Record::Agent(Event::RateLimit {
            window: Some(window),
            used: Some(used),
            ..
        }) = record
        {
            self.windows.insert(window.clone(), *used);
        }
    }
}

/// The thread header's status: "running · Limited · 5h 61% · 7d 20%".
pub fn status_line(status: &str, usage: &Usage) -> String {
    let mut parts = vec![status.to_owned()];
    if usage.limit.is_some() {
        parts.push("Limited".into());
    }
    for (window, used) in &usage.windows {
        let name = match window.as_str() {
            "five_hour" => "5h",
            "seven_day" => "7d",
            "seven_day_opus" => "7d Opus",
            other => other,
        };
        parts.push(format!("{name} {:.0}%", used * 100.));
    }
    parts.join(" · ")
}

/// [`status_line`] after `records`.
pub fn thread_status(status: &str, records: &[Record]) -> String {
    let mut usage = Usage::default();
    records.iter().for_each(|r| usage.observe(r));
    status_line(status, &usage)
}
