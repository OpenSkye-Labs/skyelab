use skyelab_agent::{Event, timeline::Record, turn_result::last_turn};

fn done(duration_ms: u64, total: f64, text: &str) -> Record {
    Record::Agent(Event::Result {
        is_error: false,
        duration_ms,
        total_cost_usd: total,
        result: Some(text.into()),
        prompts: 1,
        usage: None,
    })
}

#[test]
fn after_an_imported_turn_the_cost_is_unknown_like_the_timeline() {
    let records = [
        Record::Prompt("old".into()),
        done(0, 0.0, "imported"),
        Record::Prompt("new".into()),
        done(1200, 9.1, "now"),
    ];
    let t = last_turn(&records).expect("a turn");
    assert_eq!(t.cost_usd, 0.0, "imported baseline: cost {}", t.cost_usd);
}
