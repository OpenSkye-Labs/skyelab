//! A pending permission request shows on the thread screen until a resolved row for it arrives. Rows come from the
//! desktop's own serializer (`skyelab_agent::Event`), never a hand-written shape.

use skyelab_agent::{Decision, Event};
use skyelab_mobile::app::{event_text, pending_permissions};
use skyelab_protocol::ThreadEvent;

fn agent(seq: i64, e: &Event) -> ThreadEvent {
    ThreadEvent {
        thread_id: 1,
        seq,
        kind: "agent".into(),
        payload_json: serde_json::to_string(e).expect("serializes"),
        created_at: 0,
    }
}

#[test]
fn pending_until_resolved() {
    let ask = |id: &str| Event::Permission {
        id: id.into(),
        tool: "Bash".into(),
        input: serde_json::json!({ "command": "make  deploy" }),
    };
    let mut events = vec![agent(1, &ask("p1")), agent(2, &ask("p2"))];
    assert_eq!(event_text(&events[0]), "Asks to use Bash: make deploy");
    assert_eq!(
        pending_permissions(&events),
        [
            ("p1".to_owned(), "Bash: make deploy".to_owned()),
            ("p2".to_owned(), "Bash: make deploy".to_owned())
        ]
    );
    let resolved = Event::PermissionResolved {
        id: "p1".into(),
        decision: Decision::AllowThread,
    };
    events.push(agent(3, &resolved));
    assert_eq!(event_text(&events[2]), "Allowed for this thread");
    assert_eq!(pending_permissions(&events).len(), 1);
    assert_eq!(pending_permissions(&events)[0].0, "p2");
    assert_eq!(
        event_text(&agent(
            4,
            &Event::Stderr {
                text: "boom".into()
            }
        )),
        "boom",
        "agent output shows as its text"
    );
}
