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
        turn_cost_usd: None,
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

fn run(records: &[Record]) -> crate::workspace::thread_model::Items {
    let mut items = crate::workspace::thread_model::Items::default();
    for r in records {
        apply(&mut items, r);
    }
    items
}

#[test]
fn deltas_grow_one_row_that_the_complete_block_replaces() {
    let mut items = crate::workspace::thread_model::Items::default();
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
            total_usd: 0.0421,
            usage: Some(usage),
            at_ms: None,
            error: None,
            copied: false,
        }
    );
}

#[test]
fn a_limited_rate_limit_adds_a_row_and_an_unlimited_one_does_not() {
    let items = run(&[
        agent(Event::RateLimit {
            window: None,
            used: None,
            limited: false,
            resets_at: None,
        }),
        agent(Event::RateLimit {
            window: None,
            used: None,
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
        window: None,
        used: None,
        limited: true,
        resets_at: Some(5),
    });
    let s = limit_after(None, &limited);
    assert_eq!(s, Some(Some(5)));
    assert_eq!(limit_after(s, &Record::Prompt("x".into())), s);
    assert_eq!(limit_after(s, &result(true, None)), s, "a failure keeps it");
    assert_eq!(limit_after(s, &result(false, None)), None);
    let free = agent(Event::RateLimit {
        window: None,
        used: None,
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

/// A reply of `paras` paragraphs of ~100 bytes, then a fenced code block whose blank lines must never be cut, then
/// more paragraphs, streamed in 7-byte deltas.
fn long_reply() -> String {
    let para = |n: usize| format!("Paragraph {n}: {}\n\n", "word ".repeat(18));
    let mut text: String = (0..30).map(para).collect();
    text.push_str("```rust\nfn a() {}\n\n\nfn b() {}\n");
    text.push_str(&"let x = 1;\n\n".repeat(300));
    text.push_str("```\n\n");
    text.extend((30..60).map(para));
    text
}

fn live_parts(items: &[Item]) -> Vec<&str> {
    items
        .iter()
        .filter_map(|i| match i {
            Item::Live {
                kind: DeltaKind::Text,
                text,
            } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_long_streamed_reply_splits_at_whole_blocks_and_finishes_as_one_row() {
    let text = long_reply();
    let mut items = crate::workspace::thread_model::Items::default();
    apply(&mut items, &Record::Prompt("hi".into()));
    let mut most_parts = 0;
    for chunk in text.as_bytes().chunks(7) {
        let changed = apply(
            &mut items,
            &delta(DeltaKind::Text, std::str::from_utf8(chunk).expect("ascii")),
        );
        assert!(changed.is_some_and(|c| c >= 1), "only the reply's rows change");
        let parts = live_parts(&items);
        most_parts = most_parts.max(parts.len());
        // Every part but the growing tail is whole blocks: it ends with a blank line, outside any fence.
        for done in &parts[..parts.len() - 1] {
            assert!(done.ends_with("\n\n"), "a part ends at a block boundary");
            assert_eq!(done.matches("```").count() % 2, 0, "no part cuts a code fence");
            assert!(done.len() >= 1024, "parts aren't tiny");
        }
        assert_eq!(parts.concat(), text[..parts.concat().len()], "the parts are the reply in order");
    }
    assert!(most_parts >= 4, "a ~10 KB reply streams as several rows, got {most_parts}");
    assert_eq!(live_parts(&items).concat(), text);

    // The complete block replaces all its parts with one row.
    let changed = apply(&mut items, &blocks(vec![Block::Text { text: text.clone() }]));
    assert_eq!(changed, Some(1));
    assert_eq!(
        *items,
        vec![Item::Prompt("hi".into()), Item::Text(text)][..]
    );
}

#[test]
fn a_split_reply_cut_off_keeps_all_it_streamed() {
    let text = long_reply();
    let mut items = run(&[Record::Prompt("hi".into()), delta(DeltaKind::Text, "")]);
    for chunk in text.as_bytes().chunks(64) {
        apply(
            &mut items,
            &delta(DeltaKind::Text, std::str::from_utf8(chunk).expect("ascii")),
        );
    }
    assert!(live_parts(&items).len() > 1);
    // Stop: what streamed is stored whole, and the turn's end settles the parts into one row.
    match super::thread_model::partial(&items) {
        Some(Record::Agent(Event::Assistant { content })) => {
            assert_eq!(content[0], Block::Text { text: text.clone() })
        }
        other => panic!("no partial: {other:?}"),
    }
    assert_eq!(apply(&mut items, &result(true, None)), Some(1));
    assert_eq!(items[1], Item::Text(text));
    assert!(matches!(items[2], Item::Done { .. }));
    assert_eq!(items.len(), 3);
}

#[test]
fn a_streaming_reply_is_not_cut_inside_a_list_item_or_before_its_stderr_id() {
    // A fenced block indented under a list item continues the item: no row may start at it.
    let body = format!(
        "{}\n\n1. step\n\n   ```sh\n   make\n   ```\n{}",
        "A".repeat(1100),
        "B".repeat(1000)
    );
    // As one delta into a row already streaming, and as the first delta of a row.
    for records in [
        vec![delta(DeltaKind::Text, "x"), delta(DeltaKind::Text, &body)],
        vec![delta(DeltaKind::Text, &body)],
    ] {
        let items = run(&records);
        assert!(live_parts(&items).len() > 1, "{:?}", live_parts(&items));
        assert!(
            !live_parts(&items).iter().any(|p| p.starts_with("   ```")),
            "{:?}",
            live_parts(&items)
        );
    }

    // Stderr while a reply streams split: its id counts the reply as one row, as a reload from the store does.
    let mut items = run(&[Record::Prompt("hi".into())]);
    for chunk in long_reply().as_bytes().chunks(64) {
        apply(
            &mut items,
            &delta(DeltaKind::Text, std::str::from_utf8(chunk).expect("ascii")),
        );
    }
    assert!(live_parts(&items).len() > 1);
    apply(&mut items, &agent(Event::Stderr { text: "warn".into() }));
    assert!(
        matches!(items.last(), Some(Item::Output { id, .. }) if id == "output-2"),
        "{:?}",
        items.last()
    );
}
