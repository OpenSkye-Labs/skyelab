//! A turn the user stopped reads "Stopped after", never "Failed after"; a real failure keeps "Failed after".

use serde_json::json;
use skyelab_agent::Event;
use skyelab_agent::timeline::cards::done_verb;

fn claude_text(v: serde_json::Value) -> Option<String> {
    match Event::parse(&v.to_string()) {
        Event::Result { result, .. } => result,
        other => panic!("a result: {other:?}"),
    }
}

#[test]
fn the_footer_verb_tells_a_stop_from_a_failure() {
    assert_eq!(done_verb(false, None), "Worked for");
    assert_eq!(done_verb(true, Some("interrupted")), "Stopped after");
    assert_eq!(done_verb(true, Some(" interrupted\n")), "Stopped after");
    assert_eq!(done_verb(true, Some("API Error: 500")), "Failed after");
    assert_eq!(done_verb(true, None), "Failed after");
}

#[test]
fn claude_aborted_reads_stopped_and_an_error_after_stop_reads_failed() {
    let base = json!({"type":"result","subtype":"error_during_execution","is_error":true,"duration_ms":1,
        "errors":["Error: Request was aborted."]});
    let mut aborted = base.clone();
    aborted["terminal_reason"] = json!("aborted_tools");
    let t = claude_text(aborted);
    assert_eq!(done_verb(true, t.as_deref()), "Stopped after");
    let mut real = base;
    real["result"] = json!("API Error: 500");
    let t = claude_text(real);
    assert_eq!(done_verb(true, t.as_deref()), "Failed after");
}
