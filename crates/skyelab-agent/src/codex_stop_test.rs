use super::*;

/// The `Result` a `turn/completed` with this turn object ends in.
fn ended(turn: Value) -> Event {
    let (mut c, _) = Codex::new(Path::new("/w"), None, None, Mode::Default, Pick::default());
    c.on_line(json!({ "id": c.start_id, "result": { "thread": { "id": "thr" }, "model": "m" } }));
    let (events, _) = c.on_line(json!({ "method": "turn/completed", "params": { "turn": turn } }));
    events
        .into_iter()
        .find(|e| matches!(e, Event::Result { .. }))
        .expect("turn/completed ends in a result")
}

fn text(e: &Event) -> Option<&str> {
    match e {
        Event::Result { result, .. } => result.as_deref(),
        _ => None,
    }
}

#[test]
fn an_interrupted_turn_reads_interrupted_even_with_an_error_message() {
    let e = ended(
        json!({ "id": "t", "status": "interrupted", "error": { "message": "stream closed" } }),
    );
    assert_eq!(text(&e), Some("interrupted"));
}

#[test]
fn a_failed_turn_keeps_its_message() {
    let e =
        ended(json!({ "id": "t", "status": "failed", "error": { "message": "model overloaded" } }));
    assert_eq!(text(&e), Some("model overloaded"));
}
