//! Acceptance: in a fork's timeline, the turns copied from its source are no cost baseline. The fork's first session
//! is fresh, so its first turn's divider shows the whole running total it reported; later turns count from the
//! fork's own totals. (The stored usage does the same: skyelab-store's migration 20.)

use crate::workspace::thread_model::{Item, Record, apply, replay};
use skyelab_agent::Event;

fn result(total: f64) -> Record {
    Record::Agent(Event::Result {
        is_error: false,
        duration_ms: 5,
        total_cost_usd: total,
        result: None,
        prompts: 1,
        usage: None,
        turn_cost_usd: None,
    })
}

fn last_cost(items: &[Item]) -> f64 {
    items
        .iter()
        .rev()
        .find_map(|i| match i {
            Item::Done { cost_usd, .. } => Some(*cost_usd),
            _ => None,
        })
        .expect("a turn")
}

#[test]
fn a_forks_first_turn_counts_its_whole_total() {
    let mut items = crate::workspace::thread_model::Items::default();
    // The source's turn, copied with its time; the fork was made at 200.
    replay(&mut items, &result(0.5), 100, Some(200));
    apply(&mut items, &result(0.75));
    assert_eq!(last_cost(&items), 0.75, "not 0.25 over the copied 0.5");
    apply(&mut items, &result(1.0));
    assert_eq!(
        last_cost(&items),
        0.25,
        "the next turn counts from the fork's own total"
    );
}
