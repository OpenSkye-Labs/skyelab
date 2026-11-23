//! Proof (review): a fork after a gone session keeps the reply the user picked.

use crate::workspace::thread_extras::fork::cut;
use crate::workspace::thread_model::{Item, Record, apply};
use skyelab_agent::{Block, Event};

fn res(is_error: bool) -> Record {
    Record::Agent(Event::Result {
        is_error,
        duration_ms: 1,
        total_cost_usd: 0.0,
        result: None,
        prompts: 1,
        usage: None,
    })
}

fn say(t: &str) -> Record {
    Record::Agent(Event::Assistant {
        content: vec![Block::Text { text: t.into() }],
    })
}

#[test]
fn fork_after_a_gone_session_keeps_the_picked_reply() {
    let records = vec![
        Record::Prompt("one".into()),
        say("first answer"),
        res(false),
        Record::Prompt("two".into()),
        Record::Agent(Event::SessionGone),
        res(true),
        Record::Prompt("two".into()),
        say("second answer"),
        res(false),
    ];
    let mut items = crate::workspace::thread_model::Items::default();
    for r in &records {
        apply(&mut items, r);
    }
    let turns = items
        .iter()
        .filter(|i| matches!(i, Item::Done { .. }))
        .count();
    let events: Vec<skyelab_store::Event> = records
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let (kind, payload_json) = r.to_row();
            skyelab_store::Event {
                thread_id: 1,
                seq: i as i64 + 1,
                kind: kind.into(),
                payload_json,
                created_at: 0,
            }
        })
        .collect();
    let kept = cut(&events, turns);
    assert!(
        kept.iter()
            .any(|e| e.payload_json.contains("second answer")),
        "fork cut too early: kept {} of {} events",
        kept.len(),
        events.len()
    );
}
