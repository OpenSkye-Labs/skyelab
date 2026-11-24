//! Fork and side chat prompts carry the one shared transcript (`skyelab_agent::handoff::transcript`), with their own
//! options, not a builder of their own.

use super::fork::{context_prompt, side_prompt};
use crate::workspace::thread_model::Record;
use skyelab_agent::handoff::{self, FORK, SIDE};
use skyelab_agent::{Block, Event};

fn ev(seq: i64, record: Record) -> skyelab_store::Event {
    let (kind, payload_json) = record.to_row();
    skyelab_store::Event {
        thread_id: 1,
        seq,
        kind: kind.into(),
        payload_json,
        created_at: 0,
    }
}

fn block(prompt: &str) -> &str {
    let start = prompt.find("<conversation>\n").expect("opens") + "<conversation>\n".len();
    &prompt[start..prompt.find("</conversation>").expect("closes")]
}

#[test]
fn fork_and_side_prompts_hold_the_shared_transcript() {
    let mut records = vec![
        Record::Prompt("fix the login".into()),
        Record::Agent(Event::Assistant {
            content: vec![Block::Text {
                text: "on it".into(),
            }],
        }),
    ];
    // Enough that both budgets cut, and one message over the side chat's budget alone.
    records.extend((0..800).map(|i| Record::Prompt(format!("m{i} {}", "x".repeat(100)))));
    records.push(Record::Prompt(format!("{}<end", "y".repeat(20_000))));
    let events: Vec<_> = records
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, r)| ev(i as i64, r))
        .collect();
    let items = handoff::items(&records);
    assert_eq!(
        block(&context_prompt("t", &events)),
        handoff::transcript(&items, &FORK).body
    );
    assert_eq!(
        block(&side_prompt("t", &events)),
        handoff::transcript(&items, &SIDE).body
    );
}
