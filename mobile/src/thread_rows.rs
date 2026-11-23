//! The thread list, the desktop's flat list at phone width: Pinned, Working, then by the day each thread last
//! changed, each row with its project, account, status and preview. And the open thread's status line.

use std::collections::BTreeMap;

use skyelab_agent::Event;
use skyelab_agent::thread_list::{DAYS, day_bucket, preview_text};
use skyelab_agent::timeline::{Record, limit_after};
use skyelab_protocol::{Project, Thread};

/// Whether a thread's agent is at work (the desktop's Starting or Running).
fn working(t: &Thread) -> bool {
    matches!(t.status.as_str(), "starting" | "running")
}

/// The list's sections, empty ones left out: pinned, working, then `DAYS` by `updated_at` given where today,
/// yesterday and the 7-day window start (`skyelab_agent::thread_list::day_starts`). Newest first in each.
pub fn sections(mut threads: Vec<Thread>, starts: [i64; 3]) -> Vec<(&'static str, Vec<Thread>)> {
    threads.sort_by_key(|t| std::cmp::Reverse(t.updated_at));
    let mut shelves: Vec<(&'static str, Vec<Thread>)> = ["Pinned", "Working"]
        .into_iter()
        .chain(DAYS)
        .map(|title| (title, Vec::new()))
        .collect();
    for t in threads {
        let at = match (t.pinned, working(&t)) {
            (true, _) => 0,
            (false, true) => 1,
            _ => 2 + day_bucket(starts, t.updated_at),
        };
        shelves[at].1.push(t);
    }
    shelves.retain(|(_, ts)| !ts.is_empty());
    shelves
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
