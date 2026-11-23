//! When a snoozed thread wakes (T3's snooze presets): the menu's choices, a typed custom time, and the label a
//! snoozed row shows. Times are ms since the epoch. Times of day (tonight, 09:00) follow the local calendar across a
//! DST change; durations (`3h`, `2d`) are exact.

use chrono::{DateTime, Datelike as _, Days, NaiveDate, NaiveTime, TimeZone, Timelike as _};

/// One snooze menu entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub label: &'static str,
    pub until_ms: i64,
}

const MORNING: u32 = 9;
const EVENING: u32 = 18;

/// The menu, soonest first: in an hour, tonight (only while 18:00 is ahead), tomorrow and next Monday at 09:00.
pub fn presets<Tz: TimeZone>(now: &DateTime<Tz>) -> Vec<Choice> {
    let today = now.date_naive();
    // On a Sunday next Monday is tomorrow, which "Tomorrow" already offers: the Monday after.
    let monday = match 7 - u64::from(today.weekday().num_days_from_monday()) {
        1 => 8,
        days => days,
    };
    let mut out = vec![Choice {
        label: "In 1 hour",
        until_ms: now.timestamp_millis() + 3_600_000,
    }];
    let at = |days: u64, hour: u32| at_local(&now.timezone(), today, days, hour, 0);
    if let Some(t) = at(0, EVENING).filter(|&t| t > now.timestamp_millis()) {
        out.push(Choice {
            label: "Tonight",
            until_ms: t,
        });
    }
    out.extend(at(1, MORNING).map(|t| Choice {
        label: "Tomorrow",
        until_ms: t,
    }));
    out.extend(at(monday, MORNING).map(|t| Choice {
        label: "Next week",
        until_ms: t,
    }));
    out
}

/// A typed wake time: `45m`, `3h`, `2d`, `18:30` (its next occurrence), `2026-12-01` (09:00) or `2026-12-01 18:30`.
/// `None` when it doesn't parse or isn't in the future.
pub fn parse_custom<Tz: TimeZone>(text: &str, now: &DateTime<Tz>) -> Option<i64> {
    let text = text.trim();
    let now_ms = now.timestamp_millis();
    let tz = now.timezone();
    let today = now.date_naive();
    let until = if let Some((n, unit)) = text
        .char_indices()
        .last()
        .and_then(|(i, c)| Some((text[..i].trim().parse::<i64>().ok()?, c)))
    {
        let minutes = match unit {
            'm' => n,
            'h' => n.checked_mul(60)?,
            'd' => n.checked_mul(24 * 60)?,
            _ => return None,
        };
        now_ms.checked_add(minutes.checked_mul(60_000)?)?
    } else if let Ok(time) = NaiveTime::parse_from_str(text, "%H:%M") {
        let (h, m) = (time.hour(), time.minute());
        // A time skipped today (a DST gap) or already passed: tomorrow's.
        at_local(&tz, today, 0, h, m)
            .filter(|&t| t > now_ms)
            .or_else(|| at_local(&tz, today, 1, h, m))?
    } else if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        at_local(&tz, date, 0, MORNING, 0)?
    } else {
        let dt = chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").ok()?;
        tz.from_local_datetime(&dt).earliest()?.timestamp_millis()
    };
    (until > now_ms && until - now_ms <= MAX_AHEAD_MS).then_some(until)
}

/// How far ahead a snooze may go: the wake timer's nanoseconds overflow past about 292 years.
const MAX_AHEAD_MS: i64 = 10 * 366 * 24 * 3_600_000;

/// Hidden in the Snoozed shelf: its wake time is still ahead.
pub fn is_asleep(until: Option<i64>, now_ms: i64) -> bool {
    until.is_some_and(|u| u > now_ms)
}

/// Woke: its wake time passed and nobody has opened it since (opening clears `until`).
pub fn is_woke(until: Option<i64>, now_ms: i64) -> bool {
    until.is_some_and(|u| u <= now_ms)
}

/// How long until it wakes, T3's way: `45m`, `3h`, `2d` (rounded up), `now` once due.
pub fn wake_label(until: i64, now_ms: i64) -> String {
    let minutes = (until.saturating_sub(now_ms).max(1) + 59_999) / 60_000;
    match minutes {
        _ if until <= now_ms => "now".into(),
        m if m < 60 => format!("{m}m"),
        m if m < 24 * 60 => format!("{}h", (m + 59) / 60),
        m => format!("{}d", (m + 24 * 60 - 1) / (24 * 60)),
    }
}

/// `hour:minute` local time, `days` after `date`.
fn at_local<Tz: TimeZone>(
    tz: &Tz,
    date: NaiveDate,
    days: u64,
    hour: u32,
    minute: u32,
) -> Option<i64> {
    let day = date.checked_add_days(Days::new(days))?;
    let local = day.and_hms_opt(hour, minute, 0)?;
    Some(
        tz.from_local_datetime(&local)
            .earliest()?
            .timestamp_millis(),
    )
}

#[cfg(test)]
#[path = "snooze_test.rs"]
mod tests;
