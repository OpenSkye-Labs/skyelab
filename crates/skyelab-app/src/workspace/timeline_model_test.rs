//! The timeline model for the richer stream: streaming rows that their complete block replaces, thinking rows,
//! a todo row updated in place, structured tool input for the cards, usage on the turn divider, the limit row and
//! the session-gone note.

use super::thread_model::{Item, Record, SESSION_GONE_NOTE, apply, limit_after};
use serde_json::json;
use skyelab_agent::{Block, DeltaKind, Event, TodoStatus, Usage};

fn agent(e: Event) -> Record {
    Record::Agent(e)
}

fn delta(kind: DeltaKind, text: &str) -> Record {
    agent(Event::Delta {
        kind,
        text: text.into(),
    })
}

fn blocks(content: Vec<Block>) -> Record {
    agent(Event::Assistant { content })
}

fn result(is_error: bool, usage: Option<Usage>) -> Record {
    agent(Event::Result {
        is_error,
        duration_ms: 4200,
        total_cost_usd: 0.0421,
        result: None,
        prompts: 1,
        usage,
    })
}

fn todo_use(id: &str, first: &str) -> Block {
    Block::ToolUse {
        id: id.into(),
        name: "TodoWrite".into(),
        input: json!({ "todos": [
            { "content": "Read", "status": first },
            { "content": "Write", "status": "pending" },
        ]}),
    }
}

fn run(records: &[Record]) -> Vec<Item> {
    let mut items = Vec::new();
    for r in records {
        apply(&mut items, r);
    }
    items
}

#[test]
fn deltas_grow_one_row_that_the_complete_block_replaces() {
    let mut items = Vec::new();
    apply(&mut items, &Record::Prompt("hi".into()));
    assert_eq!(
        apply(&mut items, &delta(DeltaKind::Thinking, "Let me")),
        Some(1)
    );
    assert_eq!(
        apply(&mut items, &delta(DeltaKind::Thinking, " look")),
        Some(1)
    );
    assert_eq!(
        items[1],
        Item::Live {
            kind: DeltaKind::Thinking,
            text: "Let me look".into()
        }
    );
    // The complete thinking block replaces the row it streamed into.
    let changed = apply(
        &mut items,
        &blocks(vec![Block::Thinking {
            thinking: "Let me look.".into(),
        }]),
    );
    assert_eq!(changed, Some(1));
    assert!(matches!(&items[1], Item::Thinking { text, .. } if text == "Let me look."));
    apply(&mut items, &delta(DeltaKind::Text, "Hel"));
    apply(&mut items, &delta(DeltaKind::Text, "lo"));
    apply(
        &mut items,
        &blocks(vec![Block::Text {
            text: "Hello".into(),
        }]),
    );
    assert_eq!(items.len(), 3, "no duplicate row: {items:?}");
    assert_eq!(items[2], Item::Text("Hello".into()));
}

#[test]
fn a_block_without_deltas_is_its_own_row_and_empty_thinking_adds_nothing() {
    let items = run(&[
        blocks(vec![Block::Thinking {
            thinking: String::new(),
        }]),
        blocks(vec![Block::Thinking {
            thinking: "plan".into(),
        }]),
        blocks(vec![Block::Text { text: "ok".into() }]),
    ]);
    assert_eq!(items.len(), 2, "{items:?}");
    assert!(
        matches!(&items[0], Item::Thinking { id, text } if text == "plan" && id == "thinking-0")
    );
    assert_eq!(items[1], Item::Text("ok".into()));
}

#[test]
fn a_stopped_turn_keeps_what_streamed() {
    let items = run(&[delta(DeltaKind::Text, "half a sen"), result(true, None)]);
    assert_eq!(items[0], Item::Text("half a sen".into()));
    assert!(matches!(items[1], Item::Done { is_error: true, .. }));
}

#[test]
fn todo_writes_in_one_turn_update_one_row_and_a_new_turn_starts_another() {
    let mut items = run(&[
        Record::Prompt("go".into()),
        blocks(vec![todo_use("t1", "in_progress")]),
        blocks(vec![Block::Text {
            text: "working".into(),
        }]),
    ]);
    let changed = apply(&mut items, &blocks(vec![todo_use("t2", "completed")]));
    assert_eq!(changed, Some(1), "updated in place");
    let todos: Vec<_> = items
        .iter()
        .filter(|i| matches!(i, Item::Todo(_)))
        .collect();
    assert_eq!(todos.len(), 1, "{items:?}");
    let Item::Todo(list) = &items[1] else {
        panic!("{items:?}")
    };
    assert_eq!(list[0].status, TodoStatus::Completed);
    // Its tool result has no row to attach to and changes nothing.
    let none = apply(
        &mut items,
        &agent(Event::User {
            content: vec![Block::ToolResult {
                tool_use_id: "t2".into(),
                content: json!("ok"),
                is_error: false,
            }],
        }),
    );
    assert_eq!(none, None);
    // A follow-up sent mid-turn keeps the list; the next turn starts a new one.
    apply(&mut items, &Record::Prompt("again".into()));
    apply(&mut items, &blocks(vec![todo_use("t3", "pending")]));
    assert_eq!(
        items.iter().filter(|i| matches!(i, Item::Todo(_))).count(),
        1
    );
    apply(&mut items, &result(false, None));
    apply(&mut items, &blocks(vec![todo_use("t4", "pending")]));
    assert_eq!(
        items.iter().filter(|i| matches!(i, Item::Todo(_))).count(),
        2
    );
}

#[test]
fn tool_rows_keep_their_structured_input() {
    let input = json!({ "file_path": "/w/src/a.rs", "old_string": "a", "new_string": "b" });
    let items = run(&[blocks(vec![Block::ToolUse {
        id: "e".into(),
        name: "Edit".into(),
        input: input.clone(),
    }])]);
    let Item::Tool(t) = &items[0] else {
        panic!("{items:?}")
    };
    assert_eq!(t.raw, input);
}

#[test]
fn the_turn_divider_carries_usage_and_cost() {
    let usage = Usage {
        input: 12_300,
        output: 175,
    };
    let items = run(&[result(false, Some(usage))]);
    assert_eq!(
        items[0],
        Item::Done {
            is_error: false,
            duration_ms: 4200,
            cost_usd: 0.0421,
            usage: Some(usage)
        }
    );
}

#[test]
fn a_limited_rate_limit_adds_a_row_and_an_unlimited_one_does_not() {
    let items = run(&[
        agent(Event::RateLimit {
            limited: false,
            resets_at: None,
        }),
        agent(Event::RateLimit {
            limited: true,
            resets_at: Some(1_791_804_000),
        }),
    ]);
    assert_eq!(
        items,
        vec![Item::Limit {
            resets_at: Some(1_791_804_000)
        }]
    );
}

#[test]
fn the_limit_state_follows_rate_limits_and_clears_on_success() {
    let limited = agent(Event::RateLimit {
        limited: true,
        resets_at: Some(5),
    });
    let s = limit_after(None, &limited);
    assert_eq!(s, Some(Some(5)));
    assert_eq!(limit_after(s, &Record::Prompt("x".into())), s);
    assert_eq!(limit_after(s, &result(true, None)), s, "a failure keeps it");
    assert_eq!(limit_after(s, &result(false, None)), None);
    let free = agent(Event::RateLimit {
        limited: false,
        resets_at: None,
    });
    assert_eq!(limit_after(s, &free), None);
}

#[test]
fn a_gone_session_adds_a_note_and_hides_its_failure() {
    let items = run(&[
        Record::Prompt("more".into()),
        agent(Event::SessionGone),
        result(true, None),
        // The stderr of the same failure says it again: still one note.
        agent(Event::Stderr {
            text: "No conversation found with session ID: x".into(),
        }),
    ]);
    assert_eq!(
        items,
        vec![
            Item::Prompt("more".into()),
            Item::Notice {
                text: SESSION_GONE_NOTE.into(),
                explains_failure: false,
            }
        ]
    );
}
