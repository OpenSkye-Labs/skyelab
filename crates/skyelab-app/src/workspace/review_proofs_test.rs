//! Review findings, as red tests until fixed: a follow-up sent mid-stream, a failed rerun after a gone session, and a
//! link in a proposed plan.

use super::thread_model::{ApprovalItem, Item, Record, apply};
use super::worktree_thread_test::{fixture, open_thread};
use serde_json::json;
use skyelab_agent::{Block, DeltaKind, Event};

fn result(is_error: bool) -> Record {
    Record::Agent(Event::Result {
        is_error,
        duration_ms: 1,
        total_cost_usd: 0.0,
        result: None,
        prompts: 1,
        usage: None,
        turn_cost_usd: None,
    })
}

#[test]
fn a_follow_up_mid_stream_leaves_no_duplicate_text() {
    let mut items = crate::workspace::thread_model::Items::default();
    for r in [
        Record::Prompt("first".into()),
        Record::Agent(Event::Delta {
            kind: DeltaKind::Text,
            text: "Hello ".into(),
        }),
        Record::Prompt("also do X".into()),
        Record::Agent(Event::Delta {
            kind: DeltaKind::Text,
            text: "world".into(),
        }),
        Record::Agent(Event::Assistant {
            content: vec![Block::Text {
                text: "Hello world".into(),
            }],
        }),
        result(false),
    ] {
        apply(&mut items, &r);
    }
    let texts: Vec<&String> = items
        .iter()
        .filter_map(|i| match i {
            Item::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        vec!["Hello world"],
        "DUPLICATE_STREAM_ROW: {items:?}"
    );
}

#[test]
fn a_failed_rerun_after_a_gone_session_still_shows_its_failure() {
    let mut items = crate::workspace::thread_model::Items::default();
    for r in [
        Record::Prompt("hi".into()),
        Record::Agent(Event::SessionGone),
        result(true),
        result(true),
    ] {
        apply(&mut items, &r);
    }
    let done = items
        .iter()
        .filter(|i| matches!(i, Item::Done { .. }))
        .count();
    assert_eq!(done, 1, "rerun failure hidden: {items:?}");
}

#[gpui::test]
fn a_plan_link_with_an_app_scheme_does_not_open(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (_ws, thread, cx) = open_thread(&f, cx);
    let plan = format!("[{}](skyelab-evil://pwn)", "click ".repeat(1500));
    let raw = json!({ "plan": plan });
    thread.update(cx, |v, cx| {
        v.items = crate::workspace::thread_model::Items::from(vec![Item::Approval(ApprovalItem {
            id: "p1".into(),
            tool: "ExitPlanMode".into(),
            summary: String::new(),
            input: raw.to_string(),
            raw,
            decision: None,
            rule: None,
        })]);
        v.lists.reset(1);
        cx.notify();
    });
    cx.run_until_parked();
    let card = cx
        .debug_bounds("plan-card")
        .expect("the plan card is drawn");
    cx.simulate_click(card.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let opened = cx.opened_url();
    assert!(
        opened.as_deref().is_none_or(|u| u.starts_with("http")),
        "PLAN_LINK_OPENED {opened:?}"
    );
}

#[test]
fn rounding_windows_paths_and_osc() {
    use super::shell_card::plain_output;
    use super::timeline_text::{divider_text, file_link, limited_text};
    assert!(limited_text(Some(7199), 0).starts_with("Usage limit reached. Retry after "));
    assert!(
        divider_text(
            "Worked for",
            1.0,
            Some(skyelab_agent::Usage {
                input: 999_950,
                output: 1
            }),
            0.0
        )
        .contains("1.0M in")
    );
    assert_eq!(
        file_link("C:/repo/a.rs:12").as_deref(),
        Some("C:/repo/a.rs")
    );
    assert_eq!(
        file_link("c:\\repo\\a.rs").as_deref(),
        Some("c:\\repo\\a.rs")
    );
    assert_eq!(file_link("x:payload"), None);
    assert_eq!(
        plain_output("\u{1b}]8;;https://x\u{7}link\u{1b}]8;;\u{1b}\\ done"),
        "link done"
    );
}
