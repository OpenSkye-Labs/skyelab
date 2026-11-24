//! A result that carries its own turn price (`turn_cost_usd`, Claude's `model_usage_since_last_result`) is stored at
//! that price, even when the running total dropped (a session resumed after Restart Agent Session).

use skyelab_store::{NewThread, Store};
use std::path::Path;

fn result(total: f64, turn: Option<f64>) -> String {
    let turn = turn.map_or(String::new(), |t| format!(r#","turn_cost_usd":{t}"#));
    format!(
        r#"{{"Result":{{"is_error":false,"duration_ms":5,"total_cost_usd":{total},"result":"ok","prompts":1,"usage":{{"input":1,"output":1}}{turn}}}}}"#
    )
}

fn costs(rows: &[(f64, Option<f64>)]) -> f64 {
    let mut s = Store::open_in_memory().expect("store");
    let p = s.create_project("p", Path::new("/p")).expect("p").id;
    let new = NewThread {
        project_id: p,
        title: "t",
        cwd: Path::new("/p"),
        account_id: "a",
        status: "idle",
    };
    let t = s.create_thread(&new).expect("thread").id;
    for (total, turn) in rows {
        s.append_event(t, "agent", &result(*total, *turn))
            .expect("e");
    }
    s.usage_days(0)
        .expect("rows")
        .iter()
        .map(|r| r.cost_usd)
        .sum()
}

#[test]
fn a_priced_turn_is_stored_at_its_own_price() {
    let total = costs(&[
        (0.5377236, None),
        (0.5469244, Some(0.0092008)),
        (0.546755, Some(0.0090306)),
    ]);
    assert!(
        (total - (0.5377236 + 0.0092008 + 0.0090306)).abs() < 1e-9,
        "{total}"
    );
}

#[test]
fn an_unpriced_turn_keeps_the_running_total_rule() {
    let total = costs(&[(0.01, None), (0.03, None)]);
    assert!((total - 0.03).abs() < 1e-9, "{total}");
}
