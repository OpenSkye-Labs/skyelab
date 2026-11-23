//! The demo thread: every kind of timeline row, a finished turn that folds, then a turn waiting on a question, a
//! plan and a permission. Used by [`crate::source::FakeSource`] (tests and the `demo` build).

use serde_json::json;
use skyelab_agent::timeline::Record;
use skyelab_agent::{Block, Event, Usage};

fn assistant(content: Vec<Block>) -> Record {
    Record::Agent(Event::Assistant { content })
}

fn tool(id: &str, name: &str, input: serde_json::Value) -> Record {
    assistant(vec![Block::ToolUse {
        id: id.into(),
        name: name.into(),
        input,
    }])
}

fn result(id: &str, content: serde_json::Value, is_error: bool) -> Record {
    Record::Agent(Event::User {
        content: vec![Block::ToolResult {
            tool_use_id: id.into(),
            content,
            is_error,
        }],
    })
}

fn ask(id: &str, tool: &str, input: serde_json::Value) -> Record {
    Record::Agent(Event::Permission {
        id: id.into(),
        tool: tool.into(),
        input,
    })
}

pub fn thread_one() -> Vec<Record> {
    vec![
        Record::Prompt("The login page redirects to / instead of the page I came from.".into()),
        assistant(vec![Block::Thinking {
            thinking: "The redirect probably drops the `next` query parameter. Check auth/login.rs first.".into(),
        }]),
        tool(
            "todo1",
            "TodoWrite",
            json!({"todos": [
                {"content": "Find where the redirect is built", "status": "completed"},
                {"content": "Keep the next parameter", "status": "in_progress"},
                {"content": "Run the auth tests", "status": "pending"}
            ]}),
        ),
        tool("bash1", "Bash", json!({"command": "cargo test -p auth"})),
        result(
            "bash1",
            json!("Exit code 101\nrunning 12 tests\ntest login::redirect_keeps_next ... FAILED\n\nfailures:\n    login::redirect_keeps_next"),
            true,
        ),
        tool(
            "edit1",
            "Edit",
            json!({"file_path": "src/auth/login.rs",
                   "old_string": "let to = \"/\";\nredirect(to)",
                   "new_string": "let to = query.next.as_deref().unwrap_or(\"/\");\nredirect(to)"}),
        ),
        result("edit1", json!("Edited"), false),
        tool(
            "agent1",
            "Agent",
            json!({"subagent_type": "Explore", "description": "Find other redirect callers"}),
        ),
        Record::Agent(Event::Task {
            tool_use_id: "agent1".into(),
            status: Some("running".into()),
            activity: None,
            summary: None,
            duration_ms: None,
        }),
        Record::Agent(Event::Nested {
            parent: "agent1".into(),
            content: vec![Block::ToolUse {
                id: "grep1".into(),
                name: "Grep".into(),
                input: json!({"pattern": "redirect("}),
            }],
        }),
        Record::Agent(Event::Task {
            tool_use_id: "agent1".into(),
            status: Some("completed".into()),
            activity: Some("Searched 41 files".into()),
            summary: Some("Only login.rs builds the redirect.".into()),
            duration_ms: Some(8_400),
        }),
        assistant(vec![Block::Text {
            text: "Found it 🔍 the redirect dropped `next`. Fixed in src/auth/login.rs and the tests pass ✅".into(),
        }]),
        Record::Agent(Event::Result {
            is_error: false,
            duration_ms: 42_300,
            total_cost_usd: 0.0421,
            result: None,
            prompts: 1,
            usage: Some(Usage {
                input: 12_300,
                output: 175,
            }),
        }),
        Record::Prompt("Now add a test for it, and screenshot the login page.".into()),
        tool("shot1", "browser_screenshot", json!({})),
        // What the desktop store leaves of a screenshot once it drops the bytes.
        result(
            "shot1",
            json!([{"type": "image", "source": {"type": "base64", "omitted_bytes": 210_039}}]),
            false,
        ),
        Record::Agent(Event::RateLimit {
            limited: false,
            resets_at: None,
            window: Some("five_hour".into()),
            used: Some(0.61),
        }),
        ask(
            "q1",
            "AskUserQuestion",
            json!({"questions": [{
                "header": "Test", "question": "Where should the test go?", "multiSelect": false,
                "options": [
                    {"label": "Unit test", "description": "Next to login.rs"},
                    {"label": "Integration test", "description": "tests/auth.rs"}
                ]
            }]}),
        ),
        ask(
            "plan1",
            "ExitPlanMode",
            json!({"plan": "1. Add tests/auth.rs with a redirect case\n2. Run the auth tests\n3. Screenshot /login"}),
        ),
        ask("perm1", "Bash", json!({"command": "make deploy"})),
    ]
}
