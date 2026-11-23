//! Claude's `total_cost_usd` is a running total, so a turn's cost is its growth over the thread's previous total.
//! The totals here are a real two-turn process (T3 Code's recorded `multi_turn` transcript, claude-sonnet-4-6) plus
//! a third turn priced the same way.

use skyelab_agent::{Event, turn_cost};

fn costs(totals: &[f64]) -> Vec<f64> {
    let mut previous = 0.0;
    totals
        .iter()
        .map(|&total| {
            let cost = turn_cost(previous, total);
            if total > 0.0 {
                previous = total;
            }
            cost
        })
        .collect()
}

fn close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

#[test]
fn three_turns_of_one_process_sum_to_the_last_total() {
    // Turn 2's own usage (3 in, 19 cache write, 11792 cache read, 7 out) prices to 0.00372285.
    let totals = [0.01171425, 0.0154371, 0.0191628];
    let per_turn = costs(&totals);
    assert!(
        close(&per_turn, &[0.01171425, 0.00372285, 0.0037257]),
        "{per_turn:?}"
    );
    assert!((per_turn.iter().sum::<f64>() - 0.0191628).abs() < 1e-9);
}

#[test]
fn a_resumed_process_that_restores_the_total_counts_only_its_growth() {
    // Claude Code 2.1.277+: the new process's first result carries the session's earlier spend.
    let per_turn = costs(&[0.01, 0.03, 0.035, 0.04]);
    assert!(
        close(&per_turn, &[0.01, 0.02, 0.005, 0.005]),
        "{per_turn:?}"
    );
}

#[test]
fn a_resumed_process_that_starts_at_zero_is_a_fresh_baseline() {
    // Older CLIs: the new process reports only its own turns; a drop marks the reset.
    let per_turn = costs(&[0.01, 0.03, 0.004, 0.009]);
    assert!(
        close(&per_turn, &[0.01, 0.02, 0.004, 0.005]),
        "{per_turn:?}"
    );
}

#[test]
fn an_aborted_turns_zero_neither_counts_nor_resets() {
    // An interrupted turn's result reports 0; the next result keeps counting from the turn before it.
    let per_turn = costs(&[0.01, 0.0, 0.015]);
    assert!(close(&per_turn, &[0.01, 0.0, 0.005]), "{per_turn:?}");
}

#[test]
fn a_results_usage_is_the_turns_own() {
    // Turn 2 of the same recording: `usage` is that turn's, `modelUsage` the running total (ignored).
    let line = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":1415,"num_turns":1,"result":"second","session_id":"s","total_cost_usd":0.0154371,"usage":{"input_tokens":3,"cache_creation_input_tokens":19,"cache_read_input_tokens":11792,"output_tokens":7},"modelUsage":{"claude-sonnet-4-6":{"inputTokens":6,"outputTokens":14,"cacheReadInputTokens":21247,"cacheCreationInputTokens":2356,"costUSD":0.0154371}}}"#;
    let Event::Result { usage, .. } = Event::parse(line) else {
        panic!("a result");
    };
    let usage = usage.expect("usage");
    assert_eq!((usage.input, usage.output), (11814, 7));
}
