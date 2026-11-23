//! The strip lists only what still runs: running delegated children, and subagents of the live turn.

use super::super::super::subagent::{SubagentItem, SubagentStatus};
use super::*;
use serde_json::json;

fn child(id: i64, title: &str, status: Status) -> ChildLink {
    ChildLink {
        id,
        title: title.to_owned().into(),
        status,
        started_ms: 0,
    }
}

fn subagent(description: &str, status: SubagentStatus) -> Item {
    let mut s = SubagentItem::new("t1", "Agent", &json!({ "description": description }));
    s.status = status;
    Item::Subagent(s)
}

#[test]
fn waits_only_on_running_children_and_live_subagents() {
    let children = [
        child(1, "Scheduled tasks", Status::Running),
        child(2, "Finished one", Status::Idle),
        child(3, "GPUI deferred", Status::Starting),
    ];
    let items = [
        subagent("Explore the repo", SubagentStatus::Running),
        subagent("Done already", SubagentStatus::Done),
    ];
    assert_eq!(
        waiting_on(&children, &items, true),
        ["Scheduled tasks", "GPUI deferred", "Explore the repo"]
    );
    assert_eq!(
        waiting_on(&children, &items, false),
        ["Scheduled tasks", "GPUI deferred"],
        "an ended turn's card waits on nothing"
    );
    let done = [child(2, "Finished one", Status::Idle)];
    assert!(
        waiting_on(&done, &items[1..], true).is_empty(),
        "nothing runs: no strip"
    );
}

#[test]
fn a_card_left_running_by_an_earlier_turn_is_not_waited_on() {
    let items = [
        Item::Prompt("first".into()),
        subagent("Never reported back", SubagentStatus::Running),
        Item::Prompt("second".into()),
        subagent("This turn's", SubagentStatus::Running),
    ];
    assert_eq!(waiting_on(&[], &items, true), ["This turn's"]);
    assert!(waiting_on(&[], &items[..3], true).is_empty());
}
