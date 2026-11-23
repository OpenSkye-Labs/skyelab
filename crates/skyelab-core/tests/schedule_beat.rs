//! Acceptance: an interval task keeps its beat. A run due at slot S that a tick starts late is followed by
//! S + k * interval (the first such slot after now), never now + interval, so ticks can't drift it.

use chrono::{DateTime, Duration, TimeZone, Utc};
use chrono_tz::America::New_York;
use skyelab_core::schedule::Schedule;

fn at(h: u32, m: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 1, h, m, s)
        .single()
        .expect("valid")
}

fn next(spec: &str, slot: DateTime<Utc>, now: DateTime<Utc>) -> DateTime<Utc> {
    let s: Schedule = spec.parse().expect("parses");
    s.next_after_run(slot, now, &New_York)
        .expect("has a next run")
}

#[test]
fn a_late_tick_does_not_move_the_beat() {
    // Due at 12:00, the tick ran it at 12:00:14.
    assert_eq!(
        next("every 30m", at(12, 0, 0), at(12, 0, 14)),
        at(12, 30, 0)
    );
    // Many late ticks in a row still land on the beat.
    let mut slot = at(12, 0, 0);
    for _ in 0..100 {
        slot = next("every 1m", slot, slot + Duration::seconds(14));
    }
    assert_eq!(slot, at(13, 40, 0));
}

#[test]
fn several_missed_slots_give_the_next_one_after_now() {
    // Due at 09:00, the app was closed until 11:10: one run now, then 12:00 (not 11:10 + 1h, not 10:00).
    assert_eq!(next("every 1h", at(9, 0, 0), at(11, 10, 0)), at(12, 0, 0));
    // Exactly on a later slot: strictly after now.
    assert_eq!(next("every 1h", at(9, 0, 0), at(11, 0, 0)), at(12, 0, 0));
}

#[test]
fn wall_times_are_read_from_now() {
    let s: Schedule = "daily 09:00".parse().expect("parses");
    let now = at(20, 0, 0);
    assert_eq!(
        s.next_after_run(at(13, 0, 0), now, &New_York),
        s.next_after(now, &New_York)
    );
}
