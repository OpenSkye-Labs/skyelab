//! Subagent cards: Claude's Agent call becomes a card that collects the subagent's own calls (nested by
//! parent_tool_use_id, a nested subagent's rolled up), takes its status, time and report from the task lines, folds
//! with its turn, and draws. The fake agent's `subagent` prompt runs the recorded frame shapes end to end.

#[cfg(unix)] // the fake agent is a shell script
use super::claude_folded_turns_test::{one_view, wait_until};
use super::subagent::{SubagentItem, SubagentStatus};
use super::thread_model::{Item, Record, apply};
#[cfg(unix)]
use gpui::TestAppContext;
use serde_json::json;
use skyelab_agent::{Block, Event};
#[cfg(unix)]
use skyelab_core::Provider;
#[cfg(unix)]
use std::path::Path;

fn agent(blocks: Vec<Block>) -> Record {
    Record::Agent(Event::Assistant { content: blocks })
}

fn call(id: &str, name: &str, input: serde_json::Value) -> Block {
    Block::ToolUse {
        id: id.into(),
        name: name.into(),
        input,
    }
}

fn result(id: &str, text: &str) -> Block {
    Block::ToolResult {
        tool_use_id: id.into(),
        content: json!(text),
        is_error: false,
    }
}

fn task(id: &str, status: Option<&str>, summary: Option<&str>, ms: Option<u64>) -> Record {
    Record::Agent(Event::Task {
        tool_use_id: id.into(),
        status: status.map(str::to_owned),
        activity: None,
        summary: summary.map(str::to_owned),
        duration_ms: ms,
    })
}

fn nested(parent: &str, content: Vec<Block>) -> Record {
    Record::Agent(Event::Nested {
        parent: parent.into(),
        content,
    })
}

fn cards(items: &[Item]) -> Vec<&SubagentItem> {
    items
        .iter()
        .filter_map(|i| match i {
            Item::Subagent(s) => Some(s),
            _ => None,
        })
        .collect()
}

#[test]
fn subagent_card_collects_nested_calls_and_status() {
    let mut items = Vec::new();
    for r in [
        Record::Prompt("audit".into()),
        agent(vec![call(
            "A",
            "Agent",
            json!({"description":"Audit math.ts","subagent_type":"Explore","prompt":"look"}),
        )]),
        task("A", Some("running"), None, None),
        nested("A", vec![]), // its prompt, a plain string
        nested(
            "A",
            vec![call("r1", "Read", json!({"file_path":"math.ts"}))],
        ),
        nested("A", vec![result("r1", "1 line")]),
        // It runs in the background: the call answers at once, and the card keeps running.
        Record::Agent(Event::User {
            content: vec![result("A", "Async agent launched")],
        }),
        // Its own subagent's call rolls up into this card.
        nested(
            "A",
            vec![call("A2", "Agent", json!({"description":"deeper"}))],
        ),
        nested("A2", vec![call("b1", "Bash", json!({"command":"ls"}))]),
        task("A2", Some("completed"), Some("leaf"), Some(10)),
    ] {
        apply(&mut items, &r);
    }
    let c = cards(&items);
    assert_eq!(c.len(), 1, "no root tool rows for the subagent's calls");
    assert!(!items.iter().any(|i| matches!(i, Item::Tool(_))));
    let s = c[0];
    assert_eq!(
        (s.kind.as_str(), s.description.as_str()),
        ("Explore", "Audit math.ts")
    );
    assert_eq!(
        s.status,
        SubagentStatus::Running,
        "a started one waits for its notification"
    );
    assert_eq!(s.summary, None);
    let names: Vec<&str> = s.tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Read", "Agent", "Bash"]);
    assert_eq!(s.tools[0].result.as_deref(), Some("1 line"));

    apply(
        &mut items,
        &task("A", Some("completed"), Some("math.ts is fine"), Some(6211)),
    );
    let s = cards(&items)[0];
    assert_eq!(s.status, SubagentStatus::Done);
    assert_eq!(s.summary.as_deref(), Some("math.ts is fine"));
    assert_eq!(super::subagent::card::status_text(s), "Done · 6s");
    // It folds with its turn like any work row.
    apply(
        &mut items,
        &agent(vec![Block::Text {
            text: "all done".into(),
        }]),
    );
    apply(
        &mut items,
        &Record::Agent(Event::Result {
            is_error: false,
            duration_ms: 1,
            total_cost_usd: 0.,
            result: None,
            prompts: 1,
            usage: None,
        }),
    );
    let card = items.iter().position(|i| matches!(i, Item::Subagent(_)));
    assert!(super::timeline_fold::folded_by(&items, card.expect("card")).is_some());
}

#[test]
fn a_task_call_without_task_lines_ends_with_its_result() {
    let mut items = Vec::new();
    apply(
        &mut items,
        &agent(vec![call(
            "T",
            "Task",
            json!({"prompt":"Find the bug\nmore"}),
        )]),
    );
    apply(
        &mut items,
        &Record::Agent(Event::User {
            content: vec![Block::ToolResult {
                tool_use_id: "T".into(),
                content: json!([{"type":"text","text":"no bug"}]),
                is_error: true,
            }],
        }),
    );
    let s = cards(&items)[0];
    assert_eq!(
        (s.kind.as_str(), s.description.as_str()),
        ("Task", "Find the bug")
    );
    assert_eq!(s.status, SubagentStatus::Failed);
    assert_eq!(s.summary.as_deref(), Some("no bug"));
    apply(
        &mut items,
        &agent(vec![call("S", "Agent", json!({"description":"stop me"}))]),
    );
    apply(&mut items, &task("S", Some("running"), None, None));
    apply(&mut items, &task("S", Some("stopped"), None, Some(1000)));
    assert_eq!(
        cards(&items)[1].status,
        SubagentStatus::Stopped,
        "a stop isn't a failure"
    );
    // A line for a call nobody made changes nothing.
    assert_eq!(
        apply(&mut items, &task("nope", Some("completed"), None, None)),
        None
    );
}

#[cfg(unix)] // the fake agent is a shell script
#[gpui::test]
fn fake_subagent_prompt_streams_a_card(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, Some(".fake-subagent-quick"));
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("subagent", window, cx))
        .expect("started");
    let done = |cx: &mut gpui::VisualTestContext| {
        one.view.read_with(cx, |v, _| {
            let c = cards(&v.items);
            c.len() == 2 && c.iter().all(|s| s.status == SubagentStatus::Done)
        })
    };
    wait_until(cx, "both subagents done", done);
    let (summaries, tools) = one.view.read_with(cx, |v, _| {
        let c = cards(&v.items);
        (
            c.iter().map(|s| s.summary.clone()).collect::<Vec<_>>(),
            c.iter().map(|s| s.tools.len()).collect::<Vec<_>>(),
        )
    });
    assert_eq!(
        summaries,
        [
            Some("package.json has one script: test runs tsc.".to_owned()),
            Some("math.ts is one line: export const add.".to_owned())
        ]
    );
    assert_eq!(tools, [1, 1]);
    // Opened, a card draws its calls.
    one.view.update(cx, |v, cx| {
        v.items.retain(|i| matches!(i, Item::Subagent(_)));
        v.lists.reset(v.items.len());
        v.toggle_tool("toolu_sa", cx);
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("subagent-card").is_some());
    assert!(cx.debug_bounds("subagent-calls").is_some());
}
