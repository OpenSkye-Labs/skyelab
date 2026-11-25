//! Lines recorded from Claude Code 2.1.331 (hook output redacted).

use crate::Event;
use crate::claude::{Asks, Sent, parse};

fn claude(line: &str) -> Vec<Event> {
    parse(line, &Sent::default(), &Asks::default())
}

const RATE_LIMIT: &str = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1795519200,"rateLimitType":"five_hour","overageStatus":"allowed","overageResetsAt":1796083200,"isUsingOverage":false,"unifiedWindows":{"five_hour":{"utilization":0.18,"resetsAt":1795519200,"status":"allowed"},"seven_day":{"utilization":0.74,"resetsAt":1795510800,"status":"allowed"}}},"uuid":"1234d13e-f238-4587-9e48-ef61ce239158","session_id":"ef06c51f-1401-43f2-a1f4-dce8d80f1540"}"#;

#[test]
fn real_rate_limit_lines_report_each_unified_window() {
    let events = claude(RATE_LIMIT);
    let readings: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::RateLimit {
                limited, resets_at, ..
            } => e.rate_limit().map(|r| (r, *limited, *resets_at)),
            _ => None,
        })
        .collect();
    assert_eq!(
        readings,
        vec![
            (("5h", 0.18), false, Some(1795519200)),
            (("Weekly", 0.74), false, Some(1795510800)),
        ],
        "{events:?}"
    );
    // The line's own event comes last, so it is what the limit state and the store's newest reading end on.
    assert!(
        matches!(events.last(), Some(Event::RateLimit { limited: false, window: Some(w), used: None, .. }) if w == "five_hour"),
        "{events:?}"
    );

    let rejected = RATE_LIMIT
        .replacen(r#""status":"allowed""#, r#""status":"rejected""#, 1)
        .replace(
            r#""overageStatus":"allowed""#,
            r#""overageStatus":"rejected""#,
        );
    let events = claude(&rejected);
    assert!(events.len() > 1, "{events:?}");
    assert!(
        matches!(
            events.last(),
            Some(Event::RateLimit {
                limited: true,
                resets_at: Some(1795519200),
                ..
            })
        ),
        "{events:?}"
    );
}

#[test]
fn hook_lines_name_the_hook_and_lifecycle_lines_are_dropped() {
    let started = r#"{"type":"system","subtype":"hook_started","hook_id":"93365edd-95c0-41d0-9ade-6cbc9571aa70","hook_name":"SessionStart:startup","hook_event":"SessionStart","uuid":"14f0eb51-c63b-4090-bd32-575abc1e371e","session_id":"f4f7b4f2-50e3-4d5e-9f90-7f1cec0b44c0"}"#;
    let done = r#"{"type":"system","subtype":"hook_response","hook_id":"93365edd-95c0-41d0-9ade-6cbc9571aa70","hook_name":"SessionStart:startup","hook_event":"SessionStart","output":"<hook output>\n","stdout":"<hook output>\n","stderr":"","exit_code":0,"outcome":"success","uuid":"3815e27e-5891-4e6a-a02e-7fd99894c811","session_id":"f4f7b4f2-50e3-4d5e-9f90-7f1cec0b44c0"}"#;
    for (line, running) in [(started, true), (done, false)] {
        assert_eq!(
            claude(line),
            vec![Event::Hook {
                id: "93365edd-95c0-41d0-9ade-6cbc9571aa70".into(),
                name: "SessionStart:startup".into(),
                running,
            }],
            "{line}"
        );
    }
    let lifecycle = r#"{"type":"command_lifecycle","command_uuid":"68e55aff-1e72-4acb-b452-2406455dd460","state":"queued","uuid":"4e6f2338-f652-438e-9c2c-6f29f6ab917a","session_id":"f4f7b4f2-50e3-4d5e-9f90-7f1cec0b44c0"}"#;
    assert_eq!(claude(lifecycle), Vec::new());
}
