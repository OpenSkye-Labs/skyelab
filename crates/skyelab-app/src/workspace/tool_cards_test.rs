//! Each rich timeline row draws: thinking (open and streaming), streamed text, the todo checklist, an edit diff, a
//! Bash card with its output in a scroll box, copy buttons on a message and its code block, the turn divider with
//! tokens and cost, the limit and notice rows, the question and plan cards. A card's file path opens the Changes tab
//! on that file.

use super::changes_model::LineKind;
use super::claude_folded_turns_test::{One, one_view};
use super::shell_card::{exit_status, plain_output};
use super::thread_model::{ApprovalItem, Item, ToolItem};
use super::timeline_text::{divider_text, file_link, limited_text};
use super::tool_cards::line_diff;
use gpui::{TestAppContext, VisualTestContext};
use serde_json::json;
use skyelab_agent::{DeltaKind, Todo, TodoStatus, Usage};
use skyelab_core::Provider;
use std::path::Path;

fn view(cx: &mut TestAppContext) -> (One, &mut VisualTestContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    one_view(cx, Provider::Claude, bin, None)
}

/// Shows `items` (with `open` rows expanded) and draws a frame.
fn show(one: &One, cx: &mut VisualTestContext, items: Vec<Item>, open: &[&str]) {
    one.view.update(cx, |v, cx| {
        v.items = items;
        v.list.reset(v.items.len());
        for id in open {
            v.toggle_tool(id, cx);
        }
        cx.notify();
    });
    cx.run_until_parked();
}

fn drawn(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.debug_bounds(selector).is_some()
}

fn tool(
    id: &str,
    name: &str,
    raw: serde_json::Value,
    result: Option<&str>,
    is_error: bool,
) -> Item {
    Item::Tool(ToolItem {
        id: id.into(),
        name: name.into(),
        input: raw.to_string(),
        raw,
        result: result.map(str::to_owned),
        is_error,
    })
}

#[gpui::test]
fn thinking_streaming_text_and_copy_buttons_draw(cx: &mut TestAppContext) {
    let (one, cx) = view(cx);
    show(
        &one,
        cx,
        vec![
            Item::Thinking {
                id: "thinking-0".into(),
                text: "Look at the parser first.".into(),
            },
            Item::Text("Here:\n\n```rust\nfn main() {}\n```\n".into()),
            Item::Live {
                kind: DeltaKind::Text,
                text: "Streaming in".into(),
            },
        ],
        &["thinking-0"],
    );
    assert!(drawn(cx, "thinking-row"));
    assert!(drawn(cx, "message-copy"), "a message has a copy button");
    assert!(drawn(cx, "code-copy"), "a code block has a copy button");
    assert!(drawn(cx, "streaming-text"));
}

#[gpui::test]
fn todo_edit_and_bash_cards_draw(cx: &mut TestAppContext) {
    let (one, cx) = view(cx);
    show(
        &one,
        cx,
        vec![
            Item::Todo(vec![
                Todo {
                    content: "Read".into(),
                    status: TodoStatus::Completed,
                },
                Todo {
                    content: "Write".into(),
                    status: TodoStatus::InProgress,
                },
            ]),
            tool(
                "e1",
                "Edit",
                json!({ "file_path": "/w/src/a.rs", "old_string": "a\nb", "new_string": "a\nc" }),
                Some("ok"),
                false,
            ),
            tool(
                "b1",
                "Bash",
                json!({ "command": "cargo test" }),
                Some("\u{1b}[32mok\u{1b}[0m\n"),
                false,
            ),
        ],
        &["b1"],
    );
    assert!(drawn(cx, "todo-card"));
    assert!(drawn(cx, "edit-card"));
    assert!(drawn(cx, "card-path"));
    assert!(drawn(cx, "bash-card"));
    assert!(
        drawn(cx, "bash-output"),
        "an opened command shows its output"
    );
}

#[gpui::test]
fn divider_limit_notice_question_and_plan_draw(cx: &mut TestAppContext) {
    let (one, cx) = view(cx);
    let ask = |id: &str, tool: &str, raw: serde_json::Value| {
        Item::Approval(ApprovalItem {
            id: id.into(),
            tool: tool.into(),
            summary: String::new(),
            input: raw.to_string(),
            raw,
            decision: None,
        })
    };
    show(
        &one,
        cx,
        vec![
            Item::Done {
                is_error: false,
                duration_ms: 4200,
                cost_usd: 0.1667,
                usage: Some(Usage {
                    input: 269_947,
                    output: 175,
                }),
            },
            Item::Limit { resets_at: Some(1) },
            Item::Notice {
                text: "note".into(),
                explains_failure: false,
            },
            ask(
                "q",
                "AskUserQuestion",
                json!({ "questions": [{ "question": "Which?", "header": "H", "options": [{ "label": "A", "description": "a" }], "multiSelect": false }] }),
            ),
            ask("p", "ExitPlanMode", json!({ "plan": "## Plan\n\n1. Go" })),
        ],
        &[],
    );
    assert!(drawn(cx, "limit-row"));
    assert!(drawn(cx, "notice-row"));
    assert!(drawn(cx, "question-card"));
    assert!(drawn(cx, "question-option-0-0"));
    assert!(drawn(cx, "plan-card"));
    assert_eq!(
        divider_text(
            "Worked for",
            4.2,
            Some(Usage {
                input: 269_947,
                output: 175
            }),
            0.1667
        ),
        "Worked for 4.2s · 269.9k in · 175 out · $0.1667"
    );
    assert_eq!(
        divider_text("Worked for", 1.0, None, 0.0),
        "Worked for 1.0s"
    );
}

#[test]
fn small_helpers() {
    assert_eq!(
        line_diff("a\nb\nc", "a\nx\nc"),
        vec![
            (Some(1), Some(1), LineKind::Context, "a".into()),
            (Some(2), None, LineKind::Removed, "b".into()),
            (None, Some(2), LineKind::Added, "x".into()),
            (Some(3), Some(3), LineKind::Context, "c".into()),
        ]
    );
    assert_eq!(line_diff("", "new").len(), 1);
    assert_eq!(
        plain_output("\u{1b}[32mok\u{1b}[0m\nprogress 50%\rprogress 100%"),
        "ok\nprogress 100%"
    );
    let bash = |result: &str, is_error| ToolItem {
        id: "b".into(),
        name: "Bash".into(),
        input: String::new(),
        raw: json!({}),
        result: Some(result.into()),
        is_error,
    };
    assert_eq!(
        exit_status(&bash("Exit code 2\nboom", true)).as_deref(),
        Some("exit 2")
    );
    assert_eq!(exit_status(&bash("boom", true)).as_deref(), Some("failed"));
    assert_eq!(exit_status(&bash("fine", false)), None);
    assert_eq!(file_link("src/a.rs:12").as_deref(), Some("src/a.rs"));
    assert_eq!(file_link("/abs/b.rs:3:4").as_deref(), Some("/abs/b.rs"));
    assert_eq!(file_link("file:///etc/passwd"), None);
    assert_eq!(file_link("mailto:x@y.z"), None);
    assert_eq!(file_link("javascript:alert(1)"), None);
    assert_eq!(file_link("#anchor"), None);
    assert_eq!(
        limited_text(Some(100 + 8040), 100),
        "Usage limit reached. Limited until it resets in 2h 14m."
    );
    assert!(limited_text(None, 0).starts_with("Usage limit reached"));
}
