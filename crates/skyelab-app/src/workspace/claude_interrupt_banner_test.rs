//! A Claude turn the user interrupted ends with `error_during_execution`, `terminal_reason: aborted_streaming` and no
//! `result` text (T3's recorded message_steering transcript). The user asked for it: no failure banner.

use super::thread_failure::failure;
use super::thread_model::{Item, Record, apply};
use skyelab_agent::Event;

#[test]
fn claude_interrupt_shows_no_failure_banner() {
    let mut items: crate::workspace::thread_model::Items = vec![Item::Prompt("go".into())].into();
    let line = r#"{"type":"result","subtype":"error_during_execution","duration_ms":747,"is_error":true,"num_turns":1,"stop_reason":null,"terminal_reason":"aborted_streaming","session_id":"s","total_cost_usd":0,"errors":["[ede_diagnostic] result_type=user last_content_type=n/a stop_reason=null","Error: Request was aborted."]}"#;
    let event = Event::parse(line);
    assert!(
        matches!(event, Event::Result { is_error: true, .. }),
        "parses as an error result: {event:?}"
    );
    apply(&mut items, &Record::Agent(event));
    assert!(
        items.iter().any(|i| matches!(i, Item::Done { .. })),
        "the divider is there"
    );
    assert_eq!(
        failure(&items),
        None,
        "BUG: an interrupted Claude turn shows the failure banner"
    );
}
