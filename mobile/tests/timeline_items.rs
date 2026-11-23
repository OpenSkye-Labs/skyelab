//! Acceptance: the thread screen builds the desktop's own timeline model from stored rows, folds a finished turn's
//! work under one "Worked for …" header, and draws each kind of row compactly.

use gpui::TestAppContext;
use serde_json::json;
use skyelab_agent::timeline::{Item, Record};
use skyelab_agent::{Block, Event};
use skyelab_mobile::{
    app::{Connect, MobileApp},
    source::{FakeSource, ThreadSource},
    timeline_rows::row_lines,
};
use skyelab_protocol::ThreadEvent;

fn rows(records: &[Record]) -> Vec<ThreadEvent> {
    records
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let (kind, payload_json) = r.to_row();
            ThreadEvent {
                thread_id: 1,
                seq: i as i64 + 1,
                kind: kind.into(),
                payload_json,
                created_at: 0,
            }
        })
        .collect()
}

fn assistant(content: Vec<Block>) -> Record {
    Record::Agent(Event::Assistant { content })
}

fn tool(id: &str, name: &str, input: serde_json::Value) -> Block {
    Block::ToolUse {
        id: id.into(),
        name: name.into(),
        input,
    }
}

fn turn() -> Vec<Record> {
    let long_output: String = (1..=50).map(|i| format!("line {i}\n")).collect();
    let old: String = (1..=30).map(|i| format!("old {i}\n")).collect();
    let new: String = (1..=30).map(|i| format!("new {i}\n")).collect();
    vec![
        Record::Prompt("fix it".into()),
        assistant(vec![Block::Thinking {
            thinking: "Let me look at the login code.".into(),
        }]),
        assistant(vec![tool(
            "todo",
            "TodoWrite",
            json!({"todos": [{"content": "Find the bug", "status": "completed"},
                              {"content": "Fix it", "status": "in_progress"}]}),
        )]),
        assistant(vec![tool("b1", "Bash", json!({"command": "cargo test"}))]),
        Record::Agent(Event::User {
            content: vec![Block::ToolResult {
                tool_use_id: "b1".into(),
                content: json!(format!("Exit code 1\n{long_output}")),
                is_error: true,
            }],
        }),
        assistant(vec![tool(
            "e1",
            "Edit",
            json!({"file_path": "/src/login.rs", "old_string": old, "new_string": new}),
        )]),
        assistant(vec![tool(
            "a1",
            "Agent",
            json!({"subagent_type": "Explore", "description": "Find callers"}),
        )]),
        Record::Agent(Event::Task {
            tool_use_id: "a1".into(),
            status: Some("running".into()),
            activity: Some("Reading auth.rs".into()),
            summary: None,
            duration_ms: None,
        }),
        assistant(vec![Block::Text {
            text: "Fixed the redirect.".into(),
        }]),
    ]
}

fn result() -> Record {
    Record::Agent(Event::Result {
        is_error: false,
        duration_ms: 4_200,
        total_cost_usd: 0.0,
        result: Some("Fixed the redirect.".into()),
        prompts: 1,
        usage: None,
    })
}

struct Rows(Vec<ThreadEvent>);
impl ThreadSource for Rows {
    fn connection(&self) -> skyelab_mobile::source::Connection {
        skyelab_mobile::source::Connection::Connected
    }
    fn error(&self) -> Option<String> {
        None
    }
    fn projects(&self) -> Vec<skyelab_protocol::Project> {
        FakeSource::new().projects()
    }
    fn threads(&self) -> Vec<skyelab_protocol::Thread> {
        FakeSource::new().threads()
    }
    fn events(&self, _: i64) -> &[ThreadEvent] {
        &self.0
    }
    fn load_threads(&mut self) {}
    fn watch(&mut self, _: i64) {}
    fn send_message(&mut self, _: i64, _: &str) {}
    fn interrupt(&mut self, _: i64) {}
}

fn open(
    cx: &mut TestAppContext,
    records: Vec<Record>,
) -> (gpui::Entity<MobileApp>, &mut gpui::VisualTestContext) {
    cx.update(gpui_component::init);
    let dir = tempfile::tempdir().expect("temp dir");
    let data = dir.path().to_path_buf();
    let events = rows(&records);
    let connect: Connect =
        Box::new(move |_: &str| Ok(Box::new(Rows(events.clone())) as Box<dyn ThreadSource>));
    let (app, cx) = cx.add_window_view(|window, cx| MobileApp::new(data, connect, window, cx));
    let input = app.read_with(cx, |a, _| a.env_input.clone());
    input.update_in(cx, |s, window, cx| {
        s.set_value("laptop.tail1234.ts.net", window, cx)
    });
    app.update_in(cx, |a, window, cx| a.connect(window, cx));
    app.update_in(cx, |a, window, cx| a.open_thread(1, window, cx));
    cx.run_until_parked();
    (app, cx)
}

#[gpui::test]
fn a_finished_turn_folds_its_work(cx: &mut TestAppContext) {
    let mut records = turn();
    records.push(result());
    let (app, cx) = open(cx, records);
    app.update(cx, |a, _| a.sync_items());
    app.read_with(cx, |a, _| {
        let kinds: Vec<&str> = a
            .items()
            .iter()
            .map(|i| match i {
                Item::Prompt(_) => "prompt",
                Item::Thinking { .. } => "thinking",
                Item::Todo(_) => "todo",
                Item::Tool(_) => "tool",
                Item::Subagent(_) => "subagent",
                Item::Text(_) => "text",
                Item::Done { .. } => "done",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "prompt", "thinking", "todo", "tool", "tool", "subagent", "text", "done"
            ]
        );
        // Every work row is hidden, the first one under the header; the reply and prompt show.
        assert_eq!(a.fold_header(1).as_deref(), Some("Worked for 4.2s"));
        for ix in 1..=5 {
            assert!(a.row_hidden(ix), "row {ix} folds");
        }
        assert!(
            !a.row_hidden(0) && !a.row_hidden(6),
            "prompt and reply stay"
        );
    });
    app.update(cx, |a, cx| a.toggle("fold:7", cx));
    app.read_with(cx, |a, _| assert!(!a.row_hidden(3), "open after a tap"));
}

#[gpui::test]
fn a_running_turn_folds_nothing(cx: &mut TestAppContext) {
    let (app, cx) = open(cx, turn());
    app.update(cx, |a, _| a.sync_items());
    app.read_with(cx, |a, _| {
        assert!((0..a.items().len()).all(|ix| !a.row_hidden(ix)));
        assert_eq!(a.fold_header(1), None);
    });
}

#[test]
fn rows_are_compact() {
    let mut items = Vec::new();
    for r in turn() {
        skyelab_agent::timeline::apply(&mut items, &r);
    }
    let text = |ix: usize, open: bool| row_lines(&items[ix], open).join("\n");

    assert_eq!(text(1, false), "Thinking ›", "thinking starts closed");
    assert!(text(1, true).contains("Let me look at the login code."));

    let todo = text(2, false);
    assert!(
        todo.contains("✓ Find the bug") && todo.contains("◐ Fix it"),
        "{todo}"
    );

    let shell = row_lines(&items[3], true);
    assert_eq!(shell[0], "$ cargo test · exit 1");
    assert!(
        shell.len() <= 1 + 20 + 1,
        "output clipped: {} lines",
        shell.len()
    );
    assert_eq!(row_lines(&items[3], false), ["$ cargo test · exit 1"]);

    let edit = row_lines(&items[4], false);
    assert_eq!(edit[0], "Edit /src/login.rs +30 −30");
    let changed = edit
        .iter()
        .filter(|l| l.starts_with('+') || l.starts_with('-'))
        .count();
    assert!(changed <= 12, "at most 12 diff lines, got {changed}");

    let agent = text(5, false);
    assert!(
        agent.contains("Explore")
            && agent.contains("Find callers")
            && agent.contains("Running")
            && agent.contains("Reading auth.rs"),
        "{agent}"
    );
}
