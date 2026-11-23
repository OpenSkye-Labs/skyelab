//! Acceptance 1: schedule math in a real time zone, including both DST changes, and cron parsing.

use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::America::New_York;
use skyelab_core::schedule::Schedule;

fn ny(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
    New_York
        .with_ymd_and_hms(y, mo, d, h, mi, 0)
        .earliest()
        .expect("valid local time")
        .with_timezone(&Utc)
}

fn next(spec: &str, after: DateTime<Utc>) -> DateTime<Utc> {
    let s: Schedule = spec.parse().expect("parses");
    s.next_after(after, &New_York).expect("has a next run")
}

#[test]
fn intervals_add_elapsed_time() {
    let t = ny(2026, 3, 7, 12, 0);
    assert_eq!(next("every 30m", t), t + chrono::Duration::minutes(30));
    assert_eq!(next("every 2h", t), t + chrono::Duration::hours(2));
    // Across spring-forward night an interval is still elapsed time, not wall time.
    let night = ny(2026, 3, 8, 1, 30);
    assert_eq!(next("every 1h", night), night + chrono::Duration::hours(1));
}

#[test]
fn daily_keeps_wall_time_across_dst() {
    // A day before each change, then the run after it: 09:00 local both times, though the offset changed.
    assert_eq!(
        next("daily 09:00", ny(2026, 3, 7, 12, 0)),
        ny(2026, 3, 8, 9, 0)
    );
    assert_eq!(
        next("daily 09:00", ny(2026, 10, 31, 12, 0)),
        ny(2026, 11, 1, 9, 0)
    );
    assert_eq!(
        next("daily 09:00", ny(2026, 3, 8, 9, 0)) - ny(2026, 3, 8, 9, 0),
        chrono::Duration::hours(24)
    );
}

#[test]
fn a_time_skipped_or_repeated_by_dst_runs_once() {
    // 02:30 doesn't exist on 2026-03-08 in New York: it runs once, at 03:00 EDT.
    let first = next("daily 02:30", ny(2026, 3, 7, 12, 0));
    assert_eq!(first, ny(2026, 3, 8, 3, 0));
    assert_eq!(next("daily 02:30", first), ny(2026, 3, 9, 2, 30));
    // 01:30 happens twice on 2026-11-01: one run, then the next day's.
    let first = next("daily 01:30", ny(2026, 10, 31, 12, 0));
    let second = next("daily 01:30", first);
    assert_eq!(second, ny(2026, 11, 2, 1, 30));
}

#[test]
fn weekdays_skip_the_weekend() {
    // Saturday noon → Monday 09:00; Monday 09:00 → Tuesday 09:00.
    assert_eq!(
        next("weekdays 09:00", ny(2026, 3, 7, 12, 0)),
        ny(2026, 3, 9, 9, 0)
    );
    assert_eq!(
        next("weekdays 09:00", ny(2026, 3, 9, 9, 0)),
        ny(2026, 3, 10, 9, 0)
    );
    assert_eq!(
        next("weekdays 09:00", ny(2026, 3, 13, 9, 0)),
        ny(2026, 3, 16, 9, 0)
    );
}

#[test]
fn cron_uses_five_fields() {
    assert_eq!(
        next("cron 0 9 * * 1-5", ny(2026, 3, 7, 12, 0)),
        ny(2026, 3, 9, 9, 0)
    );
    assert_eq!(
        next("cron */15 * * * *", ny(2026, 3, 7, 12, 1)),
        ny(2026, 3, 7, 12, 15)
    );
}

#[test]
fn bad_schedules_are_refused_with_a_reason() {
    for bad in [
        "cron 0 0 9 * * *", // a seconds field: every-second runs
        "cron nonsense",
        "cron 61 * * * *",
        "every 0m",
        "every 5x",
        "daily 25:00",
        "weekdays 9",
        "hourly",
        "",
    ] {
        let err = bad.parse::<Schedule>().expect_err(bad);
        assert!(!err.to_string().is_empty(), "{bad}: a reason");
    }
    let long = format!("cron {}", "1,".repeat(200) + "1 * * * *");
    assert!(long.parse::<Schedule>().is_err(), "an oversized cron");
}

#[test]
fn specs_round_trip_and_read_as_words() {
    for spec in [
        "every 30m",
        "every 2h",
        "daily 09:00",
        "weekdays 17:30",
        "cron 0 9 * * 1-5",
    ] {
        let s: Schedule = spec.parse().expect("parses");
        assert_eq!(s.to_string(), spec);
    }
    let every_90: Schedule = "every 90m".parse().expect("parses");
    assert_eq!(every_90.to_string(), "every 90m");
    let label = |spec: &str| spec.parse::<Schedule>().expect("parses").label();
    assert_eq!(label("every 30m"), "Every 30 minutes");
    assert_eq!(label("every 1h"), "Every hour");
    assert_eq!(label("every 2h"), "Every 2 hours");
    assert_eq!(label("daily 09:00"), "Daily at 09:00");
    assert_eq!(label("weekdays 09:00"), "Weekdays at 09:00");
    assert_eq!(label("cron 0 9 * * 1-5"), "Cron 0 9 * * 1-5");
}
