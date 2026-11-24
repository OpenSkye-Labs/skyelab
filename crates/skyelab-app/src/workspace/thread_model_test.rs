use super::thread_model::{Item, Record, apply};
use skyelab_agent::Event;

fn replay(lines: &str) -> crate::workspace::thread_model::Items {
    let mut items = crate::workspace::thread_model::Items::default();
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
            ..
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
    let mut items = crate::workspace::thread_model::Items::default();
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
    let mut items = crate::workspace::thread_model::Items::default();
    assert_eq!(
        apply(&mut items, &Record::Prompt("hi\nthere".into())),
        Some(0)
    );
    assert_eq!(
        apply(&mut items, &Record::Agent(Event::parse("garbage"))),
        None
    );
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

#[test]
fn agent_links_open_only_web_pages() {
    use super::thread_render::opens_in_browser;
    for ok in [
        "https://example.com",
        "HTTP://example.com/x",
        "  https://a.b",
    ] {
        assert!(opens_in_browser(ok), "{ok}");
    }
    for bad in [
        "file:///Applications/Calculator.app",
        "javascript:alert(1)",
        "vscode://file/etc/passwd",
        "slack://open",
        "/etc/passwd",
        "httpsx://evil",
    ] {
        assert!(!opens_in_browser(bad), "{bad}");
    }
}

/// The daemon's stand-in for an event too big to send decodes as this app's own row, for both kinds.
#[test]
fn oversize_remote_stand_in_decodes_as_a_row() {
    let big = "x".repeat(skyelab_daemon::MAX_REPLY_BYTES + 1);
    for record in [
        Record::Prompt(big.clone()),
        Record::Agent(Event::Assistant {
            content: vec![skyelab_agent::Block::Text { text: big.clone() }],
        }),
    ] {
        let (kind, payload) = record.to_row();
        let wire = skyelab_daemon::event(skyelab_store::Event {
            thread_id: 1,
            seq: 7,
            kind: kind.into(),
            payload_json: payload,
            created_at: 0,
        });
        assert_eq!(wire.seq, 7);
        let shown = match Record::from_row(&wire.kind, &wire.payload_json).expect("decodes") {
            Record::Prompt(text) => text,
            Record::Agent(Event::Assistant { content }) => match &content[..] {
                [skyelab_agent::Block::Text { text }] => text.clone(),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        };
        assert!(shown.contains("too large"), "{kind}: {shown}");
    }
}

#[test]
fn each_divider_shows_what_its_turn_added_to_claudes_running_total() {
    let result = |total: f64| {
        format!(
            r#"{{"type":"result","is_error":false,"duration_ms":5,"total_cost_usd":{total},"result":"x"}}"#
        )
    };
    // Three turns of one process, an interrupted one (0), then a resumed older CLI that starts over.
    let lines = [0.01, 0.025, 0.04, 0.0, 0.05, 0.003].map(result).join("\n");
    let costs: Vec<f64> = replay(&lines)
        .iter()
        .filter_map(|i| match i {
            Item::Done { cost_usd, .. } => Some(*cost_usd),
            _ => None,
        })
        .collect();
    let want = [0.01, 0.015, 0.015, 0.0, 0.01, 0.003];
    assert!(
        costs.len() == want.len() && costs.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-9),
        "{costs:?}"
    );
}

#[test]
fn a_resumed_imported_sessions_restored_total_is_not_its_first_turns_cost() {
    // The row `skyelab_agent::import` stores for a transcript turn, through the real serializer.
    let imported = Record::Agent(Event::Result {
        is_error: false,
        duration_ms: 0,
        total_cost_usd: 0.0,
        result: None,
        prompts: 1,
        usage: None,
        turn_cost_usd: None,
    });
    let live = |total: f64| {
        Record::Agent(Event::parse(&format!(
            r#"{{"type":"result","is_error":false,"duration_ms":5,"total_cost_usd":{total},"result":"x"}}"#
        )))
    };
    let mut items = crate::workspace::thread_model::Items::default();
    let mut store = skyelab_store::Store::open_in_memory().expect("store");
    let p = store
        .create_project("p", std::path::Path::new("/p"))
        .expect("p")
        .id;
    let new = skyelab_store::NewThread {
        project_id: p,
        title: "t",
        cwd: std::path::Path::new("/p"),
        account_id: "a",
        status: "idle",
    };
    let (kind, row) = imported.to_row();
    let t = store
        .import_thread(&new, "sess", 1_000, &[(kind, row, 1_000)])
        .expect("import")
        .expect("new")
        .id;
    apply(&mut items, &imported);
    // `--resume` restores the session's $9.00 before a $0.10 turn, then a $0.20 turn.
    for record in [live(9.1), live(9.3)] {
        apply(&mut items, &record);
        let (kind, row) = record.to_row();
        store.append_event(t, kind, &row).expect("event");
    }
    let costs: Vec<f64> = items
        .iter()
        .filter_map(|i| match i {
            Item::Done { cost_usd, .. } => Some(*cost_usd),
            _ => None,
        })
        .collect();
    assert!(
        (costs[1]).abs() < 1e-9 && (costs[2] - 0.2).abs() < 1e-9,
        "{costs:?}"
    );
    let stored: f64 = store
        .usage_days(0)
        .expect("rows")
        .iter()
        .map(|r| r.cost_usd)
        .sum();
    assert!((stored - 0.2).abs() < 1e-9, "{stored}");
}
