//! Acceptance: a wall-time task saved in one time zone moves to the new zone's slot once the machine's zone changes.
//! In an unchanged zone the stored next run is exactly `next_after(now)`, so nothing moves.

use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use skyelab_core::schedule::Schedule;

fn zone(hours: i32) -> FixedOffset {
    FixedOffset::east_opt(hours * 3600).expect("valid offset")
}

fn utc(d: u32, h: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, d, h, 0, 0)
        .single()
        .expect("valid")
}

#[test]
fn a_zone_change_moves_a_daily_task() {
    let s: Schedule = "daily 09:00".parse().expect("parses");
    let saved_at = utc(1, 0);
    // Saved in UTC+8: 09:00 there is 01:00 UTC.
    let stored = s.next_after(saved_at, &zone(8)).expect("next");
    assert_eq!(stored, utc(1, 1));
    // Still in UTC+8 later: nothing to move.
    assert_eq!(s.rezoned(stored, utc(1, 0), &zone(8)), None);
    // Now in UTC-5 (09:00 there is 14:00 UTC): it moves to that.
    assert_eq!(s.rezoned(stored, utc(1, 0), &zone(-5)), Some(utc(1, 14)));
}

#[test]
fn intervals_and_due_tasks_stay() {
    let every: Schedule = "every 2h".parse().expect("parses");
    assert_eq!(every.rezoned(utc(1, 5), utc(1, 4), &zone(-5)), None);
    // A run already due runs; its next slot is read in the new zone then.
    let daily: Schedule = "daily 09:00".parse().expect("parses");
    assert_eq!(daily.rezoned(utc(1, 1), utc(1, 2), &zone(-5)), None);
}
