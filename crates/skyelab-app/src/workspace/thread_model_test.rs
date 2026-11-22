use super::thread_model::{Item, Record, apply};
use skyelab_agent::Event;

fn replay(lines: &str) -> Vec<Item> {
    let mut items = Vec::new();
    for line in lines.lines() {
        apply(&mut items, &Record::Agent(Event::parse(line)));
    }
    items
}

#[test]
fn assistant_text_tool_calls_and_result_map_to_items() {
    let items = replay(include_str!("../../tests/fixtures/session.jsonl"));
    let texts: Vec<_> = items
        .iter()
        .filter_map(|i| match i {
            Item::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 2, "{items:?}");
    assert!(texts[1].starts_with("## Summary"), "markdown kept as-is");

    let tools: Vec<_> = items
        .iter()
        .filter_map(|i| match i {
            Item::Tool(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].name, "Bash");
    assert!(tools[0].input.contains("ls crates"));
    assert_eq!(
        tools[0].result.as_deref(),
        Some("skyelab-agent\nskyelab-app\nskyelab-core\nskyelab-store"),
        "a tool result attaches to its tool row"
    );
    assert!(
        tools[1]
            .result
            .as_deref()
            .is_some_and(|r| r.contains("UI-free")),
        "array content is flattened to its text"
    );

    match items.last() {
        Some(Item::Done {
            is_error,
            duration_ms,
            cost_usd,
        }) => {
            assert!(!is_error);
            assert_eq!(*duration_ms, 4210);
            assert!((cost_usd - 0.0421).abs() < 1e-9);
        }
        other => panic!("last item should be the result, got {other:?}"),
    }
}

#[test]
fn a_tool_result_updates_the_row_it_belongs_to() {
    let mut items = Vec::new();
    let use_line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}},{"type":"tool_use","id":"t2","name":"Read","input":{}}]}}"#;
    apply(&mut items, &Record::Agent(Event::parse(use_line)));
    let result = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t2","content":"ok","is_error":true}]}}"#;
    let changed = apply(&mut items, &Record::Agent(Event::parse(result)));
    assert_eq!(changed, Some(1), "only the second row changed");
    let Item::Tool(t2) = &items[1] else {
        panic!("tool row")
    };
    assert_eq!(t2.result.as_deref(), Some("ok"));
    assert!(t2.is_error);
    let Item::Tool(t1) = &items[0] else {
        panic!("tool row")
    };
    assert_eq!(t1.result, None);
}

#[test]
fn prompts_show_and_unknown_lines_do_not() {
    let mut items = Vec::new();
    assert_eq!(
        apply(&mut items, &Record::Prompt("hi\nthere".into())),
        Some(0)
    );
    assert_eq!(apply(&mut items, &Record::Agent(Event::parse("garbage"))), None);
    assert_eq!(items, vec![Item::Prompt("hi\nthere".into())]);
}

#[test]
fn records_round_trip_through_the_store_row() {
    for record in [
        Record::Prompt("hello".into()),
        Record::Agent(Event::parse(
            r#"{"type":"result","is_error":false,"duration_ms":5,"total_cost_usd":0.5,"result":"x"}"#,
        )),
        Record::Agent(Event::parse(r#"{"type":"rate_limit_event"}"#)),
    ] {
        let (kind, payload) = record.to_row();
        assert_eq!(
            Record::from_row(kind, &payload).expect("parse back"),
            record
        );
    }
}
