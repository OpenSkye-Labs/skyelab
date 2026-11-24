//! Claude Code 2.1.331 prices each turn in `model_usage_since_last_result`. After Restart Agent Session the resumed
//! running total left out the killed process's last turn (0.5469244, then 0.546755 after a ~$0.009 turn), which the
//! running-total rule read as a fresh start and charged whole.

use skyelab_agent::{Event, timeline::Record, turn_result::last_turn};

/// A real result line (trimmed to the fields the parser reads, plus the per-turn object).
const REAL: &str = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":4886,"result":"done","total_cost_usd":0.2961912,"usage":{"input_tokens":6,"cache_creation_input_tokens":192,"cache_read_input_tokens":63411,"output_tokens":176},"modelUsage":{"claude-opus-5-5":{"costUSD":0.2961912,"contextWindow":1000000}},"model_usage_since_last_result":{"claude-opus-5-5":{"inputTokens":6,"outputTokens":176,"cacheReadInputTokens":63411,"cacheCreationInputTokens":192,"costUSD":0.0177622,"contextWindow":1000000},"claude-haiku-5-5":{"costUSD":0.001}}}"#;

fn turn_cost_of(line: &str) -> Option<f64> {
    match Event::parse(line) {
        Event::Result { turn_cost_usd, .. } => turn_cost_usd,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_real_result_carries_its_turns_own_cost() {
    let cost = turn_cost_of(REAL).expect("priced");
    assert!((cost - 0.0187622).abs() < 1e-9, "{cost}");
    let without = REAL.replace("model_usage_since_last_result", "other");
    assert_eq!(turn_cost_of(&without), None);
    // A row stored before the field still loads.
    let old: Event = serde_json::from_str(
        r#"{"Result":{"is_error":false,"duration_ms":5,"total_cost_usd":0.1,"result":"ok"}}"#,
    )
    .expect("old row");
    assert!(
        matches!(
            old,
            Event::Result {
                turn_cost_usd: None,
                ..
            }
        ),
        "{old:?}"
    );
}

fn done(total: f64, turn: Option<f64>) -> Record {
    Record::Agent(Event::Result {
        is_error: false,
        duration_ms: 900,
        total_cost_usd: total,
        result: Some("ok".into()),
        prompts: 1,
        usage: None,
        turn_cost_usd: turn,
    })
}

#[test]
fn a_restarted_sessions_turn_costs_its_own_price() {
    let records = [
        Record::Prompt("reply with done".into()),
        done(0.5469244, Some(0.0092008)),
        Record::Prompt("reply with again".into()),
        done(0.546755, Some(0.0090306)),
    ];
    let t = last_turn(&records).expect("a turn");
    assert!((t.cost_usd - 0.0090306).abs() < 1e-9, "{}", t.cost_usd);

    // Without the per-turn price the running-total rule still applies.
    let records = [
        Record::Prompt("a".into()),
        done(0.01, None),
        Record::Prompt("b".into()),
        done(0.03, None),
    ];
    let t = last_turn(&records).expect("a turn");
    assert!((t.cost_usd - 0.02).abs() < 1e-9, "{}", t.cost_usd);
}
