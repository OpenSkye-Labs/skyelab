//! A row keeps its key while rows go in above it, while it streams and settles, and while its turn folds and opens,
//! so element ids and per-row state keyed by it stay with the row.

use super::thread_model::{Item, Items, Record, apply};
use super::timeline_fold::{fold_key, rows_of};
use skyelab_agent::{Block, DeltaKind, Event};

fn agent(e: Event) -> Record {
    Record::Agent(e)
}

fn text_delta(text: &str) -> Record {
    agent(Event::Delta {
        kind: DeltaKind::Text,
        text: text.into(),
    })
}

fn result() -> Record {
    agent(Event::Result {
        is_error: false,
        duration_ms: 4200,
        total_cost_usd: 0.0,
        result: None,
        prompts: 1,
        usage: None,
    })
}

fn tool_use(id: &str) -> Record {
    agent(Event::Assistant {
        content: vec![Block::ToolUse {
            id: id.into(),
            name: "Bash".into(),
            input: serde_json::json!({ "command": "ls" }),
        }],
    })
}

#[test]
fn keys_survive_deltas_settling_and_a_splice() {
    let mut items = Items::default();
    apply(&mut items, &Record::Prompt("go".into()));
    apply(&mut items, &tool_use("t1"));
    apply(&mut items, &text_delta("Hal"));
    let live = items.key(2);
    apply(&mut items, &text_delta("f"));
    assert_eq!(items.key(2), live, "a delta keeps the streaming row's key");
    apply(&mut items, &Record::Steer("and tests".into()));
    let steer = items.key(3);
    // Stopped mid-stream: the text settles in place and a note goes in under it, above the steer.
    apply(
        &mut items,
        &agent(Event::Assistant {
            content: vec![
                Block::Text {
                    text: "Half".into(),
                },
                Block::Partial,
            ],
        }),
    );
    assert!(matches!(items[2], Item::Text(_)), "{items:?}");
    assert_eq!(items.key(2), live, "settling keeps the key");
    assert!(matches!(items[3], Item::Interrupted), "{items:?}");
    assert_eq!(items[4], Item::Steer("and tests".into()));
    assert_eq!(items.key(4), steer, "the steer moved down with its key");
    assert_eq!(items.position(steer), Some(4));
    let keys: Vec<u64> = (0..items.len()).map(|ix| items.key(ix)).collect();
    let mut unique = keys.clone();
    unique.dedup();
    assert_eq!(unique.len(), keys.len(), "every row has its own key");
    assert!(keys.iter().all(|k| *k != 0));
}

#[test]
fn a_fold_key_follows_its_turn_when_rows_go_in_above() {
    let mut items = Items::default();
    apply(&mut items, &Record::Prompt("go".into()));
    apply(&mut items, &tool_use("t1"));
    apply(&mut items, &result());
    let key = fold_key(&items, 2);
    assert_eq!(rows_of(&items, &key), Some(1..3));
    items.insert(
        0,
        Item::Notice {
            text: "note".into(),
            explains_failure: false,
        },
    );
    assert_eq!(fold_key(&items, 3), key, "the divider kept its key");
    assert_eq!(
        rows_of(&items, &key),
        Some(2..4),
        "the toggle reaches the moved turn"
    );
    let rebuilt: Items = items.iter().cloned().collect::<Vec<_>>().into();
    assert_ne!(
        fold_key(&rebuilt, 3),
        key,
        "a list built again never reuses a key"
    );
}

#[cfg(unix)] // the thread runs on the fake agent, a shell script
#[gpui::test]
fn an_open_fold_stays_open_on_its_turn_after_a_splice(cx: &mut gpui::TestAppContext) {
    use super::claude_folded_turns_test::one_view;
    use skyelab_core::Provider;
    let bin =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    one.view.update(cx, |v, cx| {
        let mut items = Items::default();
        apply(&mut items, &Record::Prompt("go".into()));
        apply(&mut items, &tool_use("t1"));
        apply(
            &mut items,
            &Record::Agent(Event::Assistant {
                content: vec![Block::Text {
                    text: "done".into(),
                }],
            }),
        );
        apply(&mut items, &result());
        v.items = items;
        v.lists.reset(v.items.len());
        let key = fold_key(&v.items, 3);
        v.toggle_tool(&key, cx);
    });
    cx.run_until_parked();
    let open_before = one
        .view
        .read_with(cx, |v, _| v.is_expanded(&fold_key(&v.items, 3)));
    assert!(open_before);
    let toggled = one.view.read_with(cx, |v, _| v.items.key(1));
    one.view.update(cx, |v, cx| {
        v.items.insert(
            0,
            Item::Notice {
                text: "above".into(),
                explains_failure: false,
            },
        );
        v.lists.splice(0..0, 1);
        cx.notify();
    });
    cx.run_until_parked();
    one.view.read_with(cx, |v, _| {
        assert_eq!(v.items.key(2), toggled, "the tool row kept its key");
        assert!(
            v.is_expanded(&fold_key(&v.items, 4)),
            "OPEN_FOLD_MOVED: the turn is still open"
        );
    });
}
