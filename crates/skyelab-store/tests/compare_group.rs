//! Acceptance: a multi-model send's threads share a comparison group (schema 21), and each one's cost is the sum of
//! its stored turns.

use skyelab_store::{NewThread, Store};
use std::path::Path;

fn result(total: f64) -> String {
    format!(
        r#"{{"Result":{{"is_error":false,"duration_ms":5,"total_cost_usd":{total},"result":"ok","prompts":1,"usage":{{"input":1,"output":1}}}}}}"#
    )
}

#[test]
fn group_is_stored_and_costs_are_summed_per_thread() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("s.db");
    let mut s = Store::open(&path).expect("store");
    let p = s.create_project("demo", Path::new("/demo")).expect("p").id;
    let new = |title| NewThread {
        project_id: p,
        title,
        cwd: Path::new("/demo"),
        account_id: "acme",
        status: "idle",
    };
    let (a, b, c) = (
        s.create_thread(&new("a")).expect("a").id,
        s.create_thread(&new("b")).expect("b").id,
        s.create_thread(&new("c")).expect("c").id,
    );
    s.set_thread_compare_group(a, 77).expect("group");
    s.set_thread_compare_group(b, 77).expect("group");
    assert!(
        s.set_thread_compare_group(999, 77).is_err(),
        "no such thread"
    );
    // Running totals, as the agent reports them: the second turn added 0.25.
    s.append_event(a, "agent", &result(0.5)).expect("e");
    s.append_event(a, "agent", &result(0.75)).expect("e");
    s.append_event(b, "agent", &result(0.1)).expect("e");
    drop(s);

    let s = Store::open(&path).expect("reopen");
    let groups: Vec<Option<i64>> = [a, b, c]
        .iter()
        .map(|&id| s.thread(id).expect("read").expect("row").compare_group)
        .collect();
    assert_eq!(groups, [Some(77), Some(77), None]);
    let costs = s.thread_costs(&[a, b, c]).expect("costs");
    assert!((costs[&a] - 0.75).abs() < 1e-9, "{costs:?}");
    assert!((costs[&b] - 0.1).abs() < 1e-9, "{costs:?}");
    assert!(!costs.contains_key(&c), "no turns: no cost");
}
