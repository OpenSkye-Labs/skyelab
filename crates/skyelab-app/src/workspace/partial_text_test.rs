//! Text streamed in a turn that is cut off (a crash, Stop) is stored as a partial assistant message, so a reload
//! shows it, with an "(interrupted)" note.

use super::claude_folded_turns_test::{one_view, wait_until};
use super::thread_model::{Item, Record, apply};
use gpui::TestAppContext;
use skyelab_agent::{Block, Event};
use skyelab_core::Provider;
use std::path::Path;

#[gpui::test]
fn partial_text_survives_reload(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    one.view
        .update_in(cx, |v, window, cx| {
            v.submit_remote("partial please", window, cx)
        })
        .expect("started");
    wait_until(cx, "the agent ended with half an answer", |cx| {
        one.view.read_with(cx, |v, _| {
            v.session.is_none() && v.items.iter().any(|i| matches!(i, Item::Interrupted))
        })
    });
    let id = one
        .view
        .read_with(cx, |v, _| v.thread.as_ref().map(|t| t.id))
        .expect("row");
    let records: Vec<Record> = wait_records(&one.store, id, cx);
    let partial = records.iter().find_map(|r| match r {
        Record::Agent(Event::Assistant { content }) if content.contains(&Block::Partial) => {
            Some(content.clone())
        }
        _ => None,
    });
    assert_eq!(
        partial,
        Some(vec![
            Block::Text {
                text: "Half an answer".into()
            },
            Block::Partial
        ]),
        "the streamed text is stored, marked partial"
    );
    // A reload builds the thread from its stored rows: the text and its note come back.
    let mut items = crate::workspace::thread_model::Items::default();
    for r in &records {
        apply(&mut items, r);
    }
    let at = items
        .iter()
        .position(|i| matches!(i, Item::Text(t) if t == "Half an answer"))
        .expect("the partial text is shown after a reload");
    assert_eq!(items.get(at + 1), Some(&Item::Interrupted), "with its note");
}

fn wait_records(
    store: &super::SharedStore,
    id: i64,
    cx: &mut gpui::VisualTestContext,
) -> Vec<Record> {
    let read = || {
        store
            .lock()
            .expect("lock")
            .events_since(id, 0)
            .expect("events")
            .iter()
            .map(|e| Record::from_row(&e.kind, &e.payload_json).expect("parses"))
            .collect::<Vec<_>>()
    };
    wait_until(cx, "the partial message stored", |_| {
        read().iter().any(|r| {
            matches!(r, Record::Agent(Event::Assistant { content }) if content.contains(&Block::Partial))
        })
    });
    read()
}

#[test]
fn a_stored_partial_lands_after_its_text_even_behind_a_steer() {
    let mut items: crate::workspace::thread_model::Items = vec![
        Item::Prompt("go".into()),
        Item::Live {
            kind: skyelab_agent::DeltaKind::Text,
            text: "Half".into(),
        },
        Item::Steer("and tests".into()),
    ]
    .into();
    apply(
        &mut items,
        &Record::Agent(Event::Assistant {
            content: vec![
                Block::Text {
                    text: "Half".into(),
                },
                Block::Partial,
            ],
        }),
    );
    assert_eq!(
        items,
        vec![
            Item::Prompt("go".into()),
            Item::Text("Half".into()),
            Item::Interrupted,
            Item::Steer("and tests".into()),
        ]
    );
}
