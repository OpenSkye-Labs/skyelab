use super::*;
use crate::claude::{Asks, Sent, parse};

fn claude(line: &str) -> Vec<Event> {
    parse(line, &Sent::default(), &Asks::default())
}

fn readings(events: &[Event]) -> Vec<Event> {
    events
        .iter()
        .filter(|e| matches!(e, Event::Context { .. }))
        .cloned()
        .collect()
}

#[test]
fn a_claude_assistant_message_reports_its_context_tokens() {
    let line = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"id":"m1","model":"claude-opus-5-5","role":"assistant","content":[{"type":"text","text":"hi"}],"usage":{"input_tokens":3,"cache_creation_input_tokens":1200,"cache_read_input_tokens":80000,"output_tokens":40}}}"#;
    let events = claude(line);
    assert!(matches!(events[0], Event::Assistant { .. }), "{events:?}");
    assert_eq!(
        readings(&events),
        vec![Event::Context {
            tokens: Some(81_243),
            window: None,
            model: Some("claude-opus-5-5".into()),
        }]
    );
}

#[test]
fn a_subagents_message_and_a_placeholder_usage_report_nothing() {
    let nested = r#"{"type":"assistant","parent_tool_use_id":"toolu_a","message":{"role":"assistant","content":[],"usage":{"input_tokens":9000,"output_tokens":1}}}"#;
    assert_eq!(readings(&claude(nested)), vec![]);
    let empty = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"role":"assistant","content":[],"usage":{"input_tokens":0,"output_tokens":0}}}"#;
    assert_eq!(readings(&claude(empty)), vec![]);
    let none = r#"{"type":"assistant","message":{"role":"assistant","content":[]}}"#;
    assert_eq!(readings(&claude(none)), vec![]);
}

#[test]
fn a_claude_result_reports_the_window_from_model_usage() {
    let line = r#"{"type":"result","is_error":false,"duration_ms":1,"result":"ok","total_cost_usd":0,"modelUsage":{"claude-haiku-4-5":{"contextWindow":200000},"claude-opus-5-5":{"contextWindow":1000000}}}"#;
    let events = claude(line);
    assert!(
        events.iter().any(|e| matches!(e, Event::Result { .. })),
        "{events:?}"
    );
    // modelUsage is the process's running total, so it lists every model used: each window goes with its model.
    let mut windows = readings(&events);
    windows.sort_by_key(|e| match e {
        Event::Context { window, .. } => *window,
        _ => None,
    });
    assert_eq!(
        windows,
        vec![
            Event::Context {
                tokens: None,
                window: Some(200_000),
                model: Some("claude-haiku-4-5".into()),
            },
            Event::Context {
                tokens: None,
                window: Some(1_000_000),
                model: Some("claude-opus-5-5".into()),
            },
        ]
    );
    let bare = r#"{"type":"result","is_error":false,"duration_ms":1,"result":"ok","total_cost_usd":0,"modelUsage":{"m":{"inputTokens":3}}}"#;
    assert_eq!(readings(&claude(bare)), vec![]);
}

#[test]
fn codex_token_usage_reports_the_last_call_and_its_window_for_its_own_thread_only() {
    let (mut c, _) = Codex::new(Path::new("/w"), None, None, Mode::Default, Pick::default());
    c.on_line(json!({ "id": c.start_id, "result": { "thread": { "id": "thr" }, "model": "m" } }));
    let usage = |thread: &str| {
        json!({"method":"thread/tokenUsage/updated","params":{"threadId":thread,"tokenUsage":{
            "total":{"inputTokens":90000,"outputTokens":900,"totalTokens":90900},
            "last":{"inputTokens":50000,"outputTokens":500,"totalTokens":50500},
            "modelContextWindow":258400}}})
    };
    assert_eq!(
        readings(&c.on_line(usage("thr")).0),
        vec![Event::Context {
            tokens: Some(50_500),
            window: Some(258_400),
            model: None,
        }]
    );
    assert_eq!(readings(&c.on_line(usage("child")).0), vec![]);
}
