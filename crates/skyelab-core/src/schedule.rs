//! Scheduled tasks' rules: when a schedule runs next, what a tick does with a due task, and which account a run may
//! use. The app's scheduler (one per app) calls these; the store keeps the tasks.

use crate::{Account, AccountsConfig};
use chrono::{DateTime, Duration, TimeZone, Utc};
use std::fmt;
use std::path::Path;
use std::str::FromStr;

/// A run more than this late (the app was closed, the machine slept) is a missed run.
pub const MISSED_GRACE_MS: i64 = 10 * 60_000;
const MAX_CRON_LEN: usize = 256;

/// How often a task runs. Stored as its spec text: `every 30m`, `every 2h`, `daily 09:00`, `weekdays 09:00`,
/// `cron <5 fields>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schedule {
    Every { minutes: u32 },
    Daily { hour: u32, minute: u32 },
    Weekdays { hour: u32, minute: u32 },
    Cron(String),
}

impl Schedule {
    /// The first run strictly after `after`, with wall times read in `tz`. Elapsed time for intervals; for a
    /// time of day, a time DST skips runs once at the jump and a time it repeats runs once (croner's rules).
    pub fn next_after<Tz: TimeZone>(&self, after: DateTime<Utc>, tz: &Tz) -> Option<DateTime<Utc>> {
        let pattern = match self {
            Schedule::Every { minutes } => {
                return Some(after + Duration::minutes(i64::from(*minutes)));
            }
            Schedule::Daily { hour, minute } => format!("{minute} {hour} * * *"),
            Schedule::Weekdays { hour, minute } => format!("{minute} {hour} * * 1-5"),
            Schedule::Cron(p) => p.clone(),
        };
        let cron = parse_cron(&pattern).ok()?;
        let next = cron
            .find_next_occurrence(&after.with_timezone(tz), false)
            .ok()?;
        Some(next.with_timezone(&Utc))
    }

    /// The run after one that was due at `slot` and handled (run or skipped) at `now`. An interval keeps its beat: the
    /// first `slot + k * interval` after `now`, so a late tick never pushes it on. Wall times are read from `now`.
    pub fn next_after_run<Tz: TimeZone>(
        &self,
        slot: DateTime<Utc>,
        now: DateTime<Utc>,
        tz: &Tz,
    ) -> Option<DateTime<Utc>> {
        let Schedule::Every { minutes } = self else {
            return self.next_after(now, tz);
        };
        let step = Duration::minutes(i64::from(*minutes));
        let behind = (now - slot).num_milliseconds().max(0);
        let k = behind / step.num_milliseconds() + 1;
        Some(slot + step * i32::try_from(k).ok()?)
    }

    /// A wall-time task's next run as the machine's zone `tz` has it now, when that differs from `next` (the zone
    /// changed since it was computed). In an unchanged zone no slot lies between the save and `next`, so the first one
    /// after `now` is `next` itself. Intervals and runs already due give `None`.
    pub fn rezoned<Tz: TimeZone>(
        &self,
        next: DateTime<Utc>,
        now: DateTime<Utc>,
        tz: &Tz,
    ) -> Option<DateTime<Utc>> {
        if matches!(self, Schedule::Every { .. }) || next <= now {
            return None;
        }
        self.next_after(now, tz).filter(|n| *n != next)
    }

    /// The schedule in words, for the task list.
    pub fn label(&self) -> String {
        match self {
            Schedule::Every { minutes: 1 } => "Every minute".into(),
            Schedule::Every { minutes: 60 } => "Every hour".into(),
            Schedule::Every { minutes } if minutes % 60 == 0 => {
                format!("Every {} hours", minutes / 60)
            }
            Schedule::Every { minutes } => format!("Every {minutes} minutes"),
            Schedule::Daily { hour, minute } => format!("Daily at {hour:02}:{minute:02}"),
            Schedule::Weekdays { hour, minute } => format!("Weekdays at {hour:02}:{minute:02}"),
            Schedule::Cron(p) => format!("Cron {p}"),
        }
    }
}

impl fmt::Display for Schedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Schedule::Every { minutes } if minutes % 60 == 0 => {
                write!(f, "every {}h", minutes / 60)
            }
            Schedule::Every { minutes } => write!(f, "every {minutes}m"),
            Schedule::Daily { hour, minute } => write!(f, "daily {hour:02}:{minute:02}"),
            Schedule::Weekdays { hour, minute } => write!(f, "weekdays {hour:02}:{minute:02}"),
            Schedule::Cron(p) => write!(f, "cron {p}"),
        }
    }
}

impl FromStr for Schedule {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let (kind, rest) = s.trim().split_once(' ').unwrap_or((s.trim(), ""));
        let rest = rest.trim();
        match kind {
            "every" => {
                let (n, mult) = if let Some(n) = rest.strip_suffix('m') {
                    (n, 1)
                } else if let Some(n) = rest.strip_suffix('h') {
                    (n, 60)
                } else {
                    return Err(format!("\"{rest}\" isn't an interval like 30m or 2h"));
                };
                let n: u32 = n
                    .parse()
                    .map_err(|_| format!("\"{rest}\" isn't a number of minutes or hours"))?;
                match n.checked_mul(mult) {
                    Some(0) => Err("An interval must be at least one minute".into()),
                    Some(minutes) => Ok(Schedule::Every { minutes }),
                    None => Err("That interval is too long".into()),
                }
            }
            "daily" => time_of_day(rest).map(|(hour, minute)| Schedule::Daily { hour, minute }),
            "weekdays" => {
                time_of_day(rest).map(|(hour, minute)| Schedule::Weekdays { hour, minute })
            }
            "cron" => {
                let pattern = rest.split_whitespace().collect::<Vec<_>>().join(" ");
                let cron = parse_cron(&pattern)?;
                // `0 9 31 2 *` parses but never comes: saved, it would sit "not scheduled" for good.
                let from = Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).single();
                if from.is_none_or(|from| cron.find_next_occurrence(&from, false).is_err()) {
                    return Err("That cron expression never runs (no such date)".into());
                }
                Ok(Schedule::Cron(pattern))
            }
            _ => Err(format!("\"{}\" isn't a schedule", s.trim())),
        }
    }
}

fn time_of_day(s: &str) -> Result<(u32, u32), String> {
    let bad = || format!("\"{s}\" isn't a time like 09:00");
    let (h, m) = s.split_once(':').ok_or_else(bad)?;
    let (hour, minute): (u32, u32) = (h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
    if hour > 23 || minute > 59 || m.len() != 2 {
        return Err(bad());
    }
    Ok((hour, minute))
}

/// Five fields only: croner also takes a seconds field, and a task every second would flood the app.
fn parse_cron(pattern: &str) -> Result<croner::Cron, String> {
    if pattern.len() > MAX_CRON_LEN {
        return Err(format!(
            "A cron expression is at most {MAX_CRON_LEN} characters"
        ));
    }
    if pattern.split_whitespace().count() != 5 {
        return Err("A cron expression has five fields: minute hour day month weekday".into());
    }
    croner::Cron::from_str(pattern).map_err(|e| format!("Not a valid cron expression: {e}"))
}

/// What a tick does with a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    NotYet,
    Run,
    /// It was missed while the app wasn't running, and the setting says skip: only move its next run on.
    SkipMissed,
}

/// Several missed slots are one decision: a late task runs once (or is skipped), then counts on from now.
pub fn due(next_run_ms: Option<i64>, now_ms: i64, skip_missed: bool) -> Due {
    match next_run_ms {
        Some(at) if at <= now_ms => {
            if skip_missed && now_ms - at > MISSED_GRACE_MS {
                Due::SkipMissed
            } else {
                Due::Run
            }
        }
        _ => Due::NotYet,
    }
}

/// Full access in a run nobody watches needs the task's own confirm; it is never a default.
pub fn check_mode(mode: &str, bypass_confirmed: bool) -> Result<(), String> {
    if mode == "bypass" && !bypass_confirmed {
        return Err("Full access needs this task's own confirm.".into());
    }
    Ok(())
}

/// The account a run uses: `folder`'s account now, and only if it is the one the task was saved under. Never a
/// default: an unbound or rebound folder refuses, and the caller pauses the task.
pub fn run_account<'a>(
    config: &'a AccountsConfig,
    folder: &Path,
    saved: &str,
) -> Result<&'a Account, String> {
    match config.resolve(folder) {
        Some(a) if a.id == saved => Ok(a),
        Some(a) => Err(format!(
            "Its folder is now bound to {}, not {saved}. Edit and save it to switch.",
            a.id
        )),
        None => Err(format!(
            "Its folder isn't bound to an account (saved under {saved})."
        )),
    }
}
