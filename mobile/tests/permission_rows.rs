//! A pending permission request stays waiting on the thread screen until a resolved row for it arrives. Rows come
//! from the desktop's own serializer (`skyelab_agent::Event`), never a hand-written shape.

use skyelab_agent::timeline::{Item, Items, Record, apply};
use skyelab_agent::{Decision, Event};
use skyelab_mobile::timeline_rows::row_lines;

fn pending(items: &[Item]) -> Vec<&str> {
    items
        .iter()
        .filter_map(|i| match i {
            Item::Approval(a) if a.decision.is_none() => Some(a.id.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn pending_until_resolved() {
    let ask = |id: &str| {
        Record::Agent(Event::Permission {
            id: id.into(),
            tool: "Bash".into(),
            input: serde_json::json!({ "command": "make  deploy" }),
        })
    };
    let mut items = Items::default();
    apply(&mut items, &ask("p1"));
    apply(&mut items, &ask("p2"));
    assert_eq!(row_lines(&items[0], false), ["Bash: make deploy"]);
    assert_eq!(pending(&items), ["p1", "p2"]);
    let resolved = Record::Agent(Event::PermissionResolved {
        id: "p1".into(),
        decision: Decision::AllowThread,
        rule: None,
    });
    apply(&mut items, &resolved);
    assert_eq!(pending(&items), ["p2"]);
    apply(
        &mut items,
        &Record::Agent(Event::Stderr {
            text: "boom".into(),
        }),
    );
    assert_eq!(
        row_lines(items.last().expect("a row"), true),
        ["Agent output ⌄", "boom"],
        "agent output shows as its text, opened"
    );
}
