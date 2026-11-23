//! The Details tab's usage rows only show what still holds: each bar dated by its own reading, a window past its
//! reset marked as reset, and a "Limited" that names no reset dropped once it is old.

use super::{limit_text, usage_from};
use crate::workspace::thread_model::Record;
use skyelab_agent::Event;
use skyelab_core::{Account, Provider};
use skyelab_store::UsageReading;

const HOUR_MS: i64 = 3600 * 1000;

fn account() -> Account {
    Account {
        id: "work".into(),
        label: "Work".into(),
        provider: Provider::Claude,
        config_dir: "/tmp/work".into(),
        gh_config_dir: None,
        ssh_agent: false,
    }
}

/// A stored row as the store's trigger keeps it, from the real serializer.
fn reading(window: &str, event: Event, at_ms: i64) -> UsageReading {
    let (_, payload_json) = Record::Agent(event).to_row();
    UsageReading {
        window: window.into(),
        payload_json,
        created_at: at_ms,
    }
}

fn window(name: &str, used: f32, resets_at: Option<i64>) -> Event {
    Event::RateLimit {
        limited: false,
        resets_at,
        window: Some(name.into()),
        used: Some(used),
    }
}

#[test]
fn usage_bars_are_dated_by_their_own_reading() {
    let now = 1_800_000_000_000;
    let readings = [
        reading(
            "five_hour",
            window("five_hour", 0.3, None),
            now - 2 * HOUR_MS,
        ),
        reading(
            "seven_day",
            window("seven_day", 0.6, None),
            now - 3 * HOUR_MS,
        ),
        // The newest row of any kind is a status-only line: it dates neither bar.
        reading(
            "",
            Event::RateLimit {
                limited: false,
                resets_at: None,
                window: None,
                used: None,
            },
            now,
        ),
    ];
    let usage = usage_from(&readings, &account());
    let dates: Vec<(&str, i64)> = usage.windows.iter().map(|w| (w.label, w.at_ms)).collect();
    assert_eq!(
        dates,
        vec![("5h", now - 2 * HOUR_MS), ("Weekly", now - 3 * HOUR_MS)]
    );
}

#[test]
fn a_window_past_its_reset_reads_reset() {
    let now_s = 1_800_000_000;
    let readings = [
        reading("five_hour", window("five_hour", 0.95, Some(now_s - 60)), 0),
        reading("seven_day", window("seven_day", 0.4, Some(now_s + 3600)), 0),
    ];
    let usage = usage_from(&readings, &account());
    let reset: Vec<(&str, bool)> = usage
        .windows
        .iter()
        .map(|w| (w.label, w.is_reset(now_s)))
        .collect();
    assert_eq!(reset, vec![("5h", true), ("Weekly", false)]);
}

#[test]
fn limited_without_reset_expires_after_six_hours() {
    let now = 1_800_000_000_000;
    let limited = |at_ms| {
        let readings = [reading(
            "",
            Event::RateLimit {
                limited: true,
                resets_at: None,
                window: None,
                used: None,
            },
            at_ms,
        )];
        let u = usage_from(&readings, &account());
        limit_text(u.limited, u.limited_ms, now)
    };
    assert_eq!(limited(now - 5 * HOUR_MS).as_deref(), Some("Limited"));
    assert_eq!(
        limited(now - 7 * HOUR_MS),
        None,
        "a stale Limited stops showing"
    );
}

#[test]
fn limited_with_a_reset_says_when_and_ends_at_it() {
    let now = 1_000_000_000;
    let now_ms = now * 1000;
    assert_eq!(
        limit_text(Some(Some(now + 2 * 3600 + 14 * 60)), Some(0), now_ms).as_deref(),
        Some("Limited · resets in 2h 14m"),
        "a named reset holds however old the reading"
    );
    assert_eq!(
        limit_text(Some(Some(now)), Some(now_ms), now_ms),
        None,
        "already reset"
    );
    assert_eq!(limit_text(None, None, now_ms), None);
}
