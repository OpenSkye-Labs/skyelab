use super::*;
use serde_json::json;

fn item(summary: &str, raw: serde_json::Value) -> ApprovalItem {
    tool_item("Bash", summary, raw)
}

fn tool_item(tool: &str, summary: &str, raw: serde_json::Value) -> ApprovalItem {
    ApprovalItem {
        id: "1".into(),
        tool: tool.into(),
        summary: summary.into(),
        input: serde_json::to_string_pretty(&raw).expect("json"),
        raw,
        decision: None,
    }
}

#[test]
fn a_plain_command_shows_its_description_not_its_json() {
    let bash = item(
        "make deploy",
        json!({"command": "make deploy", "description": "Deploy the site"}),
    );
    assert_eq!(
        detail(&bash, PREVIEW_LINES),
        Detail::Note("Deploy the site".into())
    );
    assert_eq!(
        detail(
            &item("make deploy", json!({"command": "make deploy"})),
            PREVIEW_LINES
        ),
        Detail::Nothing
    );
    // Anything more (a timeout, a sandbox flag, another tool's input) keeps the whole input in view.
    let more = json!({"command": "make deploy", "dangerouslyDisableSandbox": true});
    assert_eq!(
        detail(&item("make deploy", more), PREVIEW_LINES),
        Detail::Json
    );
    // A summary that isn't the command (clipped, say) shows the input as sent.
    assert_eq!(
        detail(
            &item("make", json!({"command": "make deploy"})),
            PREVIEW_LINES
        ),
        Detail::Json
    );
    // Control and invisible characters keep the escaped JSON in view.
    assert_eq!(
        detail(
            &item("ls\u{1b}[2K", json!({"command": "ls\u{1b}[2K"})),
            PREVIEW_LINES
        ),
        Detail::Json
    );
    assert_eq!(
        detail(
            &item("ls \u{202E}fdp", json!({"command": "ls \u{202E}fdp"})),
            PREVIEW_LINES
        ),
        Detail::Json
    );
}

fn preview(d: Detail) -> Preview {
    match d {
        Detail::Diff(p) => p,
        other => panic!("expected a diff, got {other:?}"),
    }
}

#[test]
fn a_big_write_shows_its_first_lines_and_how_many_more() {
    let content: String = (1..=2000).map(|i| format!("line {i}\n")).collect();
    let write = tool_item(
        "Write",
        "/r/big.rs",
        json!({"file_path": "/r/big.rs", "content": content}),
    );
    let p = preview(detail(&write, PREVIEW_LINES));
    assert_eq!(
        (p.lines.len(), p.more, p.added, p.removed),
        (20, 1980, 2000, 0)
    );
    assert_eq!(
        p.lines[0],
        (None, Some(1), LineKind::Added, "line 1".into())
    );
    // Expanded, the card draws up to the edit card's cap.
    assert_eq!(preview(detail(&write, 400)).more, 1600);
    // CRLF files are fine; a lone carriage return could overwrite what shows, so the JSON does.
    let crlf = json!({"file_path": "/r/a", "content": "a\r\nb\r\n"});
    assert!(matches!(
        detail(&tool_item("Write", "/r/a", crlf), 20),
        Detail::Diff(_)
    ));
    let cr = json!({"file_path": "/r/a", "content": "rm -rf ~\rhello"});
    assert_eq!(detail(&tool_item("Write", "/r/a", cr), 20), Detail::Json);
}

#[test]
fn an_edit_shows_old_to_new() {
    let edit = tool_item(
        "Edit",
        "/r/a.rs",
        json!({"file_path": "/r/a.rs", "old_string": "a\nb", "new_string": "a\nc", "replace_all": true}),
    );
    let p = preview(detail(&edit, PREVIEW_LINES));
    assert_eq!((p.added, p.removed, p.more, p.replace_all), (1, 1, 0, true));
    assert_eq!(p.lines[1], (Some(2), None, LineKind::Removed, "b".into()));
    let multi = tool_item(
        "MultiEdit",
        "/r/a.rs",
        json!({"file_path": "/r/a.rs", "edits": [
            {"old_string": "x", "new_string": "y"},
            {"old_string": "p", "new_string": "q"},
        ]}),
    );
    let p = preview(detail(&multi, PREVIEW_LINES));
    assert_eq!((p.added, p.removed, p.replace_all), (2, 2, false));
}

#[test]
fn a_file_change_with_anything_unusual_shows_its_json() {
    let cases = [
        // A field the diff doesn't draw.
        (
            "Write",
            "/a",
            json!({"file_path": "/a", "content": "x", "mode": "0755"}),
        ),
        (
            "Edit",
            "/a",
            json!({"file_path": "/a", "old_string": "x", "new_string": "y", "x": 1}),
        ),
        (
            "MultiEdit",
            "/a",
            json!({"file_path": "/a", "edits": [{"old_string": "x", "new_string": "y", "z": 1}]}),
        ),
        (
            "MultiEdit",
            "/a",
            json!({"file_path": "/a", "edits": [], "more": 1}),
        ),
        // A summary that isn't the whole path (clipped).
        (
            "Write",
            "/a/…",
            json!({"file_path": "/a/b", "content": "x"}),
        ),
        // Hidden characters in the path or the text.
        (
            "Write",
            "/a\u{202E}b",
            json!({"file_path": "/a\u{202E}b", "content": "x"}),
        ),
        (
            "Edit",
            "/a",
            json!({"file_path": "/a", "old_string": "x", "new_string": "y\u{200B}"}),
        ),
        (
            "Write",
            "/a",
            json!({"file_path": "/a", "content": "\u{1b}[2Kx"}),
        ),
        (
            "Write",
            "/a",
            json!({"file_path": "/a", "content": "a\u{E0041}b"}),
        ),
        (
            "Write",
            "/a",
            json!({"file_path": "/a", "content": "a\u{2028}b"}),
        ),
        (
            "Write",
            "/a",
            json!({"file_path": "/a", "content": "a\u{061C}b"}),
        ),
        // A change only to line endings would show as no change.
        (
            "Edit",
            "/a",
            json!({"file_path": "/a", "old_string": "x\r\n", "new_string": "x\n"}),
        ),
        (
            "Edit",
            "/a",
            json!({"file_path": "/a", "old_string": "a\n", "new_string": "a"}),
        ),
        // Not the shape it should be.
        (
            "Edit",
            "/a",
            json!({"file_path": "/a", "old_string": 1, "new_string": "y"}),
        ),
        (
            "Edit",
            "/a",
            json!({"file_path": "/a", "old_string": "x", "new_string": "y", "replace_all": "yes"}),
        ),
    ];
    for (tool, summary, raw) in cases {
        assert_eq!(
            detail(&tool_item(tool, summary, raw.clone()), 20),
            Detail::Json,
            "{raw}"
        );
    }
}

#[test]
fn a_hidden_character_deep_in_a_big_write_shows_the_json() {
    // Past what the card or its 20-line preview draws: the whole content is checked.
    let mut lines: Vec<String> = (1..=2000).map(|i| format!("line {i}")).collect();
    lines[1899] = "line\u{200B} 1900".into();
    let content = lines.join("\n");
    let write = tool_item(
        "Write",
        "/r/big.rs",
        json!({"file_path": "/r/big.rs", "content": content}),
    );
    assert_eq!(detail(&write, PREVIEW_LINES), Detail::Json);
    assert_eq!(patch(&write), None, "no View all either");
}

#[test]
fn view_all_holds_every_line() {
    use gpui_component::diff::DiffFile;
    let content: String = (1..=2000).map(|i| format!("line {i}\n")).collect();
    let write = tool_item(
        "Write",
        "/r/big.rs",
        json!({"file_path": "/r/big.rs", "content": content}),
    );
    let files = DiffFile::parse(&patch(&write).expect("a patch")).expect("parses");
    assert_eq!(files.len(), 1);
    assert_eq!((files[0].additions(), files[0].deletions()), (2000, 0));
    assert_eq!(files[0].path().as_ref(), "/r/big.rs");

    // Each MultiEdit edit is its own section, as the card's divider shows.
    let multi = tool_item(
        "MultiEdit",
        "/r/a.rs",
        json!({"file_path": "/r/a.rs", "edits": [
            {"old_string": "x\ny", "new_string": "x\nz"},
            {"old_string": "", "new_string": "p\nq"},
        ]}),
    );
    assert_eq!(preview(detail(&multi, PREVIEW_LINES)).breaks, vec![3]);
    let files = DiffFile::parse(&patch(&multi).expect("a patch")).expect("parses");
    let counts: Vec<_> = files
        .iter()
        .map(|f| (f.additions(), f.deletions()))
        .collect();
    assert_eq!(counts, vec![(1, 1), (2, 0)]);

    let bash = item("make", json!({"command": "make"}));
    assert_eq!(patch(&bash), None);
}
