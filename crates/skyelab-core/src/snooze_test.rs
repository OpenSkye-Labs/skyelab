//! Snooze times on a fixed clock: Wednesday 2026-11-25 14:00 at UTC+2.

use super::*;
use chrono::FixedOffset;

fn at(text: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_str(&format!("{text} +0200"), "%Y-%m-%d %H:%M %z").expect("time")
}

fn ms(text: &str) -> i64 {
    at(text).timestamp_millis()
}

#[test]
fn presets_follow_the_local_clock() {
    let now = at("2026-11-25 14:00");
    let got: Vec<(&str, i64)> = presets(&now)
        .iter()
        .map(|c| (c.label, c.until_ms))
        .collect();
    assert_eq!(
        got,
        [
            ("In 1 hour", ms("2026-11-25 15:00")),
            ("Tonight", ms("2026-11-25 18:00")),
            ("Tomorrow", ms("2026-11-26 09:00")),
            ("Next week", ms("2026-11-30 09:00")),
        ]
    );
    // Tonight is gone once 18:00 has passed; on a Monday "next week" is a week away.
    let late = presets(&at("2026-11-30 19:00"));
    assert!(late.iter().all(|c| c.label != "Tonight"));
    assert_eq!(
        late.last().map(|c| c.until_ms),
        Some(ms("2026-12-07 09:00"))
    );
}

#[test]
fn custom_times_parse_and_must_be_ahead() {
    let now = at("2026-11-25 14:00");
    assert_eq!(parse_custom("45m", &now), Some(ms("2026-11-25 14:45")));
    assert_eq!(parse_custom(" 3h ", &now), Some(ms("2026-11-25 17:00")));
    assert_eq!(parse_custom("2d", &now), Some(ms("2026-11-27 14:00")));
    assert_eq!(parse_custom("18:30", &now), Some(ms("2026-11-25 18:30")));
    assert_eq!(
        parse_custom("09:15", &now),
        Some(ms("2026-11-26 09:15")),
        "passed: tomorrow"
    );
    assert_eq!(
        parse_custom("2026-12-01", &now),
        Some(ms("2026-12-01 09:00"))
    );
    assert_eq!(
        parse_custom("2026-12-01 18:30", &now),
        Some(ms("2026-12-01 18:30"))
    );
    for bad in [
        "",
        "soon",
        "0m",
        "-2h",
        "2026-11-24 10:00",
        "25:00",
        "99999999999999999d",
    ] {
        assert_eq!(parse_custom(bad, &now), None, "{bad:?}");
    }
}

#[test]
fn a_snooze_sleeps_until_its_time_then_wakes() {
    let until = ms("2026-11-25 15:00");
    let before = until - 1;
    assert!(is_asleep(Some(until), before) && !is_woke(Some(until), before));
    assert!(!is_asleep(Some(until), until) && is_woke(Some(until), until));
    assert!(!is_asleep(None, before) && !is_woke(None, before));
    assert_eq!(wake_label(until, ms("2026-11-25 14:15")), "45m");
    assert_eq!(wake_label(until, ms("2026-11-25 12:30")), "3h");
    assert_eq!(
        wake_label(ms("2026-11-27 09:00"), ms("2026-11-25 12:30")),
        "2d"
    );
    assert_eq!(wake_label(until, until), "now");
}
