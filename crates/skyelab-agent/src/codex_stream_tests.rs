//! Codex's streaming, reasoning, plan, token usage, rate limits and user-input questions, mapped onto the same
//! events as Claude's. Frames are copied from T3 Code's recorded app-server transcripts.

use super::*;
use crate::Block;

fn open() -> Codex {
    let (mut c, _) = Codex::new(Path::new("/w"), None, None, Mode::Default, Pick::default());
    c.on_line(json!({ "id": c.start_id, "result": { "thread": { "id": "thr" }, "model": "m" } }));
    c
}

fn events(c: &mut Codex, v: Value) -> Vec<Event> {
    c.on_line(v).0
}

#[test]
fn agent_and_reasoning_deltas_stream() {
    let mut c = open();
    assert_eq!(
        events(
            &mut c,
            json!({"method":"item/agentMessage/delta","params":{"threadId":"thr","turnId":"u","itemId":"msg_1","delta":"ste"}})
        ),
        vec![Event::Delta {
            kind: DeltaKind::Text,
            text: "ste".into()
        }]
    );
    assert_eq!(
        events(
            &mut c,
            json!({"method":"item/reasoning/summaryTextDelta","params":{"itemId":"rs_1","delta":"**Inspecting**","summaryIndex":0}})
        ),
        vec![Event::Delta {
            kind: DeltaKind::Thinking,
            text: "**Inspecting**".into()
        }]
    );
}

#[test]
fn a_reasoning_summary_is_a_thinking_block_and_an_empty_one_is_hidden() {
    let mut c = open();
    let done = |summary: Value| json!({"method":"item/completed","params":{"item":{"type":"reasoning","id":"rs","summary":summary,"content":[]}}});
    assert_eq!(
        events(&mut c, done(json!(["**Waiting for agent**"]))),
        vec![Event::Assistant {
            content: vec![Block::Thinking {
                thinking: "**Waiting for agent**".into()
            }]
        }]
    );
    assert!(events(&mut c, done(json!([]))).is_empty());
}

#[test]
fn a_plan_update_is_a_todo_list_with_one_id_per_turn() {
    let mut c = open();
    let plan = |status: &str| {
        json!({"method":"turn/plan/updated","params":{"turnId":"turn_1","explanation":null,"plan":[
            {"step":"Inspect package.json","status":status},{"step":"Report","status":"pending"}]}})
    };
    let first = events(&mut c, plan("inProgress"));
    let Some(Event::Assistant { content }) = first.first() else {
        panic!("{first:?}")
    };
    let Block::ToolUse { id, name, input } = &content[0] else {
        panic!("{content:?}")
    };
    assert_eq!(
        (id.as_str(), name.as_str()),
        ("plan-turn_1", crate::TODO_TOOL)
    );
    let todos = crate::todos(input);
    assert_eq!(todos[0].status, crate::TodoStatus::InProgress);
    assert_eq!(todos[1].content, "Report");
    let second = events(&mut c, plan("completed"));
    assert!(matches!(&second[0], Event::Assistant { content }
        if matches!(&content[0], Block::ToolUse { id, .. } if id == "plan-turn_1")));
}

#[test]
fn a_turns_tokens_are_the_totals_it_added() {
    let mut c = open();
    let usage = |input: u64, output: u64| json!({"method":"thread/tokenUsage/updated","params":{"tokenUsage":{"total":{"inputTokens":input,"outputTokens":output}}}});
    c.on_line(usage(1000, 10));
    c.on_line(json!({"method":"turn/started","params":{"turn":{"id":"u"}}}));
    c.on_line(usage(1500, 40));
    let done = events(
        &mut c,
        json!({"method":"turn/completed","params":{"turn":{"id":"u","status":"completed","durationMs":3}}}),
    );
    assert!(
        matches!(
            done.last(),
            Some(Event::Result {
                usage: Some(Usage {
                    input: 500,
                    output: 30
                }),
                ..
            })
        ),
        "{done:?}"
    );
}

#[test]
fn rate_limits_say_when_a_window_is_used_up() {
    let mut c = open();
    // Recorded: under the limit.
    let ok = events(
        &mut c,
        json!({"method":"account/rateLimits/updated","params":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":61,"windowDurationMins":300,"resetsAt":1777429830},"secondary":{"usedPercent":23,"windowDurationMins":10080,"resetsAt":1777959590},"rateLimitReachedType":null}}}),
    );
    assert_eq!(
        ok.last(),
        Some(&Event::RateLimit {
            window: None,
            used: None,
            limited: false,
            resets_at: Some(1777429830)
        })
    );
    let full = events(
        &mut c,
        json!({"method":"account/rateLimits/updated","params":{"rateLimits":{"primary":{"usedPercent":100,"resetsAt":2000100000},"secondary":null,"rateLimitReachedType":null}}}),
    );
    assert_eq!(
        full.last(),
        Some(&Event::RateLimit {
            window: None,
            used: None,
            limited: true,
            resets_at: Some(2000100000)
        })
    );
}

#[test]
fn codex_rate_limits_report_each_window() {
    let mut c = open();
    let reading = |window: &str, used: f32, resets_at: i64| Event::RateLimit {
        limited: false,
        resets_at: Some(resets_at),
        window: Some(window.to_owned()),
        used: Some(used),
    };
    // Recorded shape: 5h primary and weekly secondary, then the summary.
    let ok = events(
        &mut c,
        json!({"method":"account/rateLimits/updated","params":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":61,"windowDurationMins":300,"resetsAt":1777429830},"secondary":{"usedPercent":23,"windowDurationMins":10080,"resetsAt":1777959590},"rateLimitReachedType":null}}}),
    );
    assert_eq!(
        ok[..2],
        [
            reading("five_hour", 0.61, 1777429830),
            reading("seven_day", 0.23, 1777959590)
        ]
    );
    assert_eq!(ok.len(), 3);
    // No durations: primary is 5h, secondary weekly; over 100% reads as full.
    let full = events(
        &mut c,
        json!({"method":"account/rateLimits/updated","params":{"rateLimits":{"primary":{"usedPercent":130,"resetsAt":5},"secondary":null}}}),
    );
    assert_eq!(full[..1], [reading("five_hour", 1.0, 5)]);
    assert_eq!(full.len(), 2);
    // A model-specific limit and a monthly window aren't account readings; the summary still comes.
    let other = events(
        &mut c,
        json!({"method":"account/rateLimits/updated","params":{"rateLimits":{"limitId":"gpt-x","primary":{"usedPercent":10,"windowDurationMins":300}}}}),
    );
    assert_eq!(other.len(), 1);
    let monthly = events(
        &mut c,
        json!({"method":"account/rateLimits/updated","params":{"rateLimits":{"primary":{"usedPercent":10,"windowDurationMins":43200}}}}),
    );
    assert_eq!(monthly.len(), 1);
}

#[test]
fn a_user_input_request_is_a_question_answered_by_question_id() {
    let mut c = open();
    // Recorded (plan_questions).
    let (asked, writes) = c.on_line(json!({"method":"item/tool/requestUserInput","id":0,"params":{"threadId":"t","turnId":"u","itemId":"call_1","questions":[{"id":"schema_preference","header":"Fixture","question":"Should this fixture prefer strict schemas or UI flexibility?","isOther":true,"isSecret":false,"options":[{"label":"Strict schemas (Recommended)","description":"Keep it tight."},{"label":"UI flexibility","description":"Looser."}]}],"isBlocking":true}}));
    assert!(writes.is_empty(), "it waits for the user");
    let [Event::Permission { id, tool, input }] = &asked[..] else {
        panic!("{asked:?}")
    };
    assert_eq!(tool, crate::QUESTION_TOOL);
    assert_eq!(input["questions"][0]["header"], "Fixture");
    assert_eq!(
        input["questions"][0]["options"][1]["label"],
        "UI flexibility"
    );
    assert_eq!(c.answer_questions(id, &[]), None, "one answer per question");
    let reply = c
        .answer_questions(id, &[vec!["Strict schemas (Recommended)".into()]])
        .expect("open");
    // Recorded reply.
    assert_eq!(
        serde_json::from_str::<Value>(&reply).expect("json"),
        json!({"id":0,"result":{"answers":{"schema_preference":{"answers":["Strict schemas (Recommended)"]}}}})
    );
    c.forget(id);
    assert_eq!(c.answer_questions(id, &[vec!["x".into()]]), None);
}

#[test]
fn a_declined_question_answers_nothing() {
    let mut c = open();
    let (asked, _) = c.on_line(json!({"method":"item/tool/requestUserInput","id":4,"params":{"questions":[{"id":"q","question":"?","options":[]}]}}));
    let [Event::Permission { id, .. }] = &asked[..] else {
        panic!("{asked:?}")
    };
    let reply = c.answer(id, Decision::Deny).expect("open");
    assert_eq!(
        serde_json::from_str::<Value>(&reply).expect("json"),
        json!({"id":4,"result":{"answers":{}}})
    );
}

#[test]
fn a_secret_question_is_refused_and_a_childs_deltas_are_dropped() {
    let mut c = open();
    let (asked, writes) = c.on_line(json!({"method":"item/tool/requestUserInput","id":5,"params":{"questions":[{"id":"k","question":"API key?","isSecret":true,"options":[]}]}}));
    assert!(asked.is_empty(), "{asked:?}");
    assert!(serde_json::from_str::<Value>(&writes[0]).expect("json")["error"].is_object());
    assert!(
        events(
            &mut c,
            json!({"method":"item/agentMessage/delta","params":{"threadId":"child","delta":"x"}})
        )
        .is_empty()
    );
    assert_eq!(
        events(
            &mut c,
            json!({"method":"item/agentMessage/delta","params":{"threadId":"thr","delta":"x"}})
        )
        .len(),
        1
    );
}
