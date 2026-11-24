use super::*;
use serde_json::json;

fn item(summary: &str, raw: serde_json::Value) -> ApprovalItem {
    ApprovalItem {
        id: "1".into(),
        tool: "Bash".into(),
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
    assert_eq!(detail(&bash), Detail::Note("Deploy the site".into()));
    assert_eq!(
        detail(&item("make deploy", json!({"command": "make deploy"}))),
        Detail::Nothing
    );
    // Anything more (a timeout, a sandbox flag, another tool's input) keeps the whole input in view.
    let more = json!({"command": "make deploy", "dangerouslyDisableSandbox": true});
    assert_eq!(detail(&item("make deploy", more)), Detail::Json);
    let edit = json!({"file_path": "/a.rs", "old_string": "a", "new_string": "b"});
    assert_eq!(detail(&item("/a.rs", edit)), Detail::Json);
    // A summary that isn't the command (clipped, say) shows the input as sent.
    assert_eq!(
        detail(&item("make", json!({"command": "make deploy"}))),
        Detail::Json
    );
}
