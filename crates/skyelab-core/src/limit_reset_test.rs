use super::*;
use chrono::FixedOffset;

fn at(h: u32, m: u32) -> DateTime<FixedOffset> {
    // Wednesday 2026-11-25, UTC+8.
    FixedOffset::east_opt(8 * 3600)
        .expect("offset")
        .with_ymd_and_hms(2026, 11, 25, h, m, 0)
        .single()
        .expect("time")
}

#[test]
fn reset_words_follow_the_local_day() {
    let now = at(22, 0);
    let ms = |d: i64, h: i64, m: i64| now.timestamp_millis() + ((d * 24 + h) * 60 + m) * 60_000;
    assert_eq!(reset_words(ms(0, 0, 25), &now), "Resets today at 10:25 PM");
    assert_eq!(reset_words(ms(0, 1, 30), &now), "Resets today at 11:30 PM");
    // 22:00 + 4h10m is 2:10 AM the next day.
    assert_eq!(
        reset_words(ms(0, 4, 10), &now),
        "Resets tomorrow at 2:10 AM"
    );
    assert_eq!(reset_words(ms(2, 0, 0), &now), "Resets Friday at 10:00 PM");
    assert_eq!(reset_words(ms(9, 0, 0), &now), "Resets Dec 4 at 10:00 PM");
    assert_eq!(reset_words(now.timestamp_millis(), &now), "Reset passed");
    assert_eq!(reset_words(ms(0, -1, 0), &now), "Reset passed");
}

#[test]
fn the_timeline_row_names_the_same_time_as_the_banner() {
    let now = at(22, 0);
    let reset = now.timestamp_millis() + (4 * 60 + 10) * 60_000;
    assert_eq!(
        retry_text(Some(reset), &now, "Asia/Manila"),
        "Usage limit reached. Retry after tomorrow at 2:10 AM (Asia/Manila)."
    );
    let when = reset_when(reset, &now).expect("ahead");
    assert!(
        retry_text(Some(reset), &now, "Z").contains(&when),
        "one wording"
    );
    // A history row keeps its time after the reset passes: no countdown.
    let later = at(23, 0) + chrono::Duration::days(1);
    assert_eq!(
        retry_text(Some(reset), &later, "Asia/Manila"),
        "Usage limit reached. Retry after today at 2:10 AM (Asia/Manila)."
    );
    assert_eq!(
        retry_text(None, &now, "Asia/Manila"),
        "Usage limit reached."
    );
    assert!(!local_zone().is_empty());
}

#[test]
fn snooze_is_refused_once_the_reset_passed() {
    assert_eq!(snooze_until(2_000, 1_000), Some(2_000));
    assert_eq!(snooze_until(2_000, 2_000), None);
    assert_eq!(snooze_until(2_000, 3_000), None);
}

#[test]
fn a_resume_is_due_at_its_time() {
    assert!(!due(2_000, 1_999));
    assert!(due(2_000, 2_000));
    assert!(due(2_000, 9_000));
}
