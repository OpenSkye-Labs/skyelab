//! A reopened fork replays its stored records the way `ThreadView::open` does: its first own turn still counts its
//! whole total, not the growth over the copied history's.

use crate::workspace::thread_model::{Item, Record, replay};
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

#[test]
fn a_reloaded_forks_first_turn_counts_its_whole_total() {
    let mut items = crate::workspace::thread_model::Items::default();
    for (r, at) in [(result(0.5), 100), (result(0.75), 300)] {
        replay(&mut items, &r, at, Some(200));
    }
    let costs: Vec<f64> = items
        .iter()
        .filter_map(|i| match i {
            Item::Done { cost_usd, .. } => Some(*cost_usd),
            _ => None,
        })
        .collect();
    assert_eq!(costs, [0.5, 0.75], "the fork's own turn isn't 0.25");
}
