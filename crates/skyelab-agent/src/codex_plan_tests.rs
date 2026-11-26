//! Codex's own Plan: a Plan turn asks for its `plan` collaboration mode (as T3 does), leaving Plan says so on the next
//! turn, and the proposed plan it ends with (a `plan` item) is a plan card like Claude's ExitPlanMode.

use super::*;

fn parse(line: &str) -> Value {
    serde_json::from_str(line).expect("test line is JSON")
}

fn opened(mode: Mode) -> Codex {
    let (mut c, _) = Codex::new(Path::new("/w"), None, None, mode, Pick::default());
    c.on_line(
        json!({ "id": c.start_id, "result": { "thread": { "id": "thr" }, "model": "gpt-x" } }),
    );
    c
}

fn turn(c: &mut Codex, text: &str) -> Value {
    let lines = c.prompt(text, &[], false).expect("open");
    let params = parse(&lines[0])["params"].clone();
    c.on_line(json!({ "method": "turn/completed", "params": { "turn": { "id": "t", "status": "completed" } } }));
    params
}

#[test]
fn a_plan_turn_asks_for_codex_plan_mode_and_leaving_plan_clears_it() {
    let mut c = opened(Mode::Plan);
    assert_eq!(
        turn(&mut c, "plan it")["collaborationMode"],
        json!({ "mode": "plan", "settings": { "model": "gpt-x", "reasoning_effort": null } }),
        "the thread's model when none was picked"
    );
    c.set_mode(Mode::Default);
    assert_eq!(turn(&mut c, "go")["collaborationMode"]["mode"], "default");
}

#[test]
fn a_thread_never_in_plan_sends_no_collaboration_mode() {
    let mut c = opened(Mode::Default);
    assert!(turn(&mut c, "hi").get("collaborationMode").is_none());
}

#[test]
fn a_proposed_plan_is_a_plan_card() {
    let mut c = opened(Mode::Plan);
    let item = json!({ "type": "plan", "id": "item_p", "text": "1. Add it\n2. Test it" });
    let (events, _) = c.on_line(json!({ "method": "item/completed", "params": { "item": item } }));
    assert_eq!(
        events,
        vec![Event::Permission {
            id: "codex-plan-item_p".into(),
            tool: crate::PLAN_TOOL.into(),
            input: json!({ "plan": "1. Add it\n2. Test it", "artifact": null }),
        }]
    );
}
