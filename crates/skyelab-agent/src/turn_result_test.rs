use super::*;
use crate::{Block, Event};
use serde_json::json;

fn said(text: &str) -> Record {
    Record::Agent(Event::Assistant {
        content: vec![Block::Text { text: text.into() }],
    })
}

fn tool(name: &str, input: serde_json::Value) -> Record {
    Record::Agent(Event::Assistant {
        content: vec![Block::ToolUse {
            id: "t".into(),
            name: name.into(),
            input,
        }],
    })
}

fn done(is_error: bool, total: f64, result: Option<&str>) -> Record {
    Record::Agent(Event::Result {
        is_error,
        duration_ms: 1200,
        total_cost_usd: total,
        result: result.map(str::to_owned),
        prompts: 1,
        usage: None,
    })
}

#[test]
fn no_turn_until_a_result_arrives() {
    assert_eq!(last_turn(&[]), None);
    let records = [Record::Prompt("go".into()), said("working on it")];
    assert_eq!(last_turn(&records), None);
}

#[test]
fn the_last_finished_turn_with_its_files_summary_and_cost() {
    let records = [
        Record::Prompt("first".into()),
        tool("Edit", json!({"file_path": "/r/old.rs"})),
        done(false, 0.5, Some("first done")),
        Record::Prompt("second".into()),
        tool("Edit", json!({"file_path": "/r/a.rs"})),
        tool("Write", json!({"file_path": "/r/b.rs"})),
        tool("apply_patch", json!({"files": ["/r/c.rs", "/r/a.rs"]})),
        tool("NotebookEdit", json!({"notebook_path": "/r/n.ipynb"})),
        tool("Bash", json!({"command": "ls"})),
        said("All done."),
        done(false, 0.75, Some("Fixed it.")),
    ];
    let t = last_turn(&records).expect("a finished turn");
    assert!(!t.is_error);
    assert_eq!(t.summary, "Fixed it.");
    assert_eq!(t.duration_ms, 1200);
    assert!((t.cost_usd - 0.25).abs() < 1e-9, "cost {}", t.cost_usd);
    assert_eq!(
        t.files_changed,
        vec!["/r/a.rs", "/r/b.rs", "/r/c.rs", "/r/n.ipynb"]
    );
}

#[test]
fn summary_falls_back_to_the_turns_last_text() {
    let records = [
        Record::Prompt("go".into()),
        said("Looking."),
        said("It failed: no network."),
        done(true, 0.0, None),
    ];
    let t = last_turn(&records).expect("a finished turn");
    assert!(t.is_error);
    assert_eq!(t.summary, "It failed: no network.");
    assert_eq!(t.cost_usd, 0.0);
    assert!(t.files_changed.is_empty());
}

#[test]
fn a_running_turn_after_a_finished_one_still_reports_the_finished_one() {
    let records = [
        Record::Prompt("go".into()),
        done(false, 0.1, Some("one")),
        Record::Prompt("again".into()),
        tool("Edit", json!({"file_path": "/r/x.rs"})),
    ];
    let t = last_turn(&records).expect("the finished turn");
    assert_eq!(t.summary, "one");
    assert!(t.files_changed.is_empty());
}

#[test]
fn a_long_summary_keeps_its_end() {
    let long = "x".repeat(MAX_SUMMARY + 50) + "END";
    let records = [Record::Prompt("go".into()), done(false, 0.0, Some(&long))];
    let t = last_turn(&records).expect("a finished turn");
    assert!(t.summary.len() <= MAX_SUMMARY + "…".len());
    assert!(t.summary.starts_with('…') && t.summary.ends_with("END"));
}
