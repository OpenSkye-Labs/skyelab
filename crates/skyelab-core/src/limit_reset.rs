//! A usage limit's reset, as T3's limit banner says it: "Resets in 25 min", "Resets tomorrow at 2:10 AM", and the rules
//! for its two actions (resume at the reset, snooze until it). Times are ms since the epoch.

use chrono::{DateTime, Datelike as _, TimeZone, Weekday};

/// What a thread sends to continue once its limit resets (T3's `UsageLimitRecoveryWorker`).
pub const RESUME_PROMPT: &str = "Continue where you left off.";

/// The banner's reset: "Resets tomorrow at 2:10 AM", "Reset passed" once it has. Worded like the timeline's row
/// ([`retry_text`]), so the two never disagree.
pub fn reset_words<Tz: TimeZone>(reset_ms: i64, now: &DateTime<Tz>) -> String {
    match reset_when(reset_ms, now) {
        Some(when) => format!("Resets {when}"),
        None => "Reset passed".into(),
    }
}

/// When the reset comes, in local words ("today at 11:30 PM", "tomorrow at 2:10 AM"); `None` once it has passed.
pub fn reset_when<Tz: TimeZone>(reset_ms: i64, now: &DateTime<Tz>) -> Option<String> {
    (reset_ms > now.timestamp_millis())
        .then(|| when_words(reset_ms, now))
        .flatten()
}

/// The timeline's limit row, T3's history line: "Usage limit reached. Retry after tomorrow at 2:10 AM (Asia/Manila)."
/// No countdown, so it reads the same after the limit clears.
pub fn retry_text<Tz: TimeZone>(reset_ms: Option<i64>, now: &DateTime<Tz>, zone: &str) -> String {
    match reset_ms.and_then(|r| when_words(r, now)) {
        Some(when) => format!("Usage limit reached. Retry after {when} ({zone})."),
        None => "Usage limit reached.".into(),
    }
}

/// This machine's time zone ("Asia/Manila"), read once; the UTC offset when the system doesn't name one.
pub fn local_zone() -> &'static str {
    static ZONE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ZONE.get_or_init(|| {
        iana_time_zone::get_timezone()
            .unwrap_or_else(|_| chrono::Local::now().format("UTC%:z").to_string())
    })
}

/// `at_ms` as a local day and time: "today at 2:10 AM", "tomorrow", "yesterday", a weekday within the week ahead,
/// else the date ("Dec 4 at 10:00 PM").
fn when_words<Tz: TimeZone>(at_ms: i64, now: &DateTime<Tz>) -> Option<String> {
    let local = now
        .timezone()
        .timestamp_millis_opt(at_ms)
        .single()?
        .naive_local();
    let day = match (local.date() - now.date_naive()).num_days() {
        -1 => "yesterday".to_owned(),
        0 => "today".to_owned(),
        1 => "tomorrow".to_owned(),
        2..=6 => weekday(local.weekday()).to_owned(),
        _ => local.format("%b %-d").to_string(),
    };
    Some(format!("{day} at {}", local.format("%-I:%M %p")))
}

fn weekday(d: Weekday) -> &'static str {
    match d {
        Weekday::Mon => "Monday",
        Weekday::Tue => "Tuesday",
        Weekday::Wed => "Wednesday",
        Weekday::Thu => "Thursday",
        Weekday::Fri => "Friday",
        Weekday::Sat => "Saturday",
        Weekday::Sun => "Sunday",
    }
}

/// The snooze "Snooze until reset" sets: `None` once the reset has passed, which T3 refuses with a message.
pub fn snooze_until(reset_ms: i64, now_ms: i64) -> Option<i64> {
    (reset_ms > now_ms).then_some(reset_ms)
}

/// A scheduled resume is due.
pub fn due(at: i64, now_ms: i64) -> bool {
    at <= now_ms
}

#[cfg(test)]
#[path = "limit_reset_test.rs"]
mod tests;
