//! Streamed text shows while it streams and is never stored: the store keeps the complete blocks only.

use super::claude_folded_turns_test::{one_view, wait_until};
use super::thread_model::{Item, Record};
use gpui::TestAppContext;
use skyelab_agent::{Block, DeltaKind, Event};
use skyelab_core::Provider;
use std::path::Path;

#[gpui::test]
fn deltas_show_but_only_complete_blocks_are_stored(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    // The streaming rows are transient: a poll can miss them on a loaded machine, so every change is watched.
    let streamed = std::rc::Rc::new(std::cell::Cell::new((false, false)));
    let seen = streamed.clone();
    let _watch = cx.update(|_, cx| {
        cx.observe(&one.view, move |view, cx| {
            for i in &view.read(cx).items {
                if let Item::Live { kind, .. } = i {
                    let (thinking, text) = seen.get();
                    seen.set((
                        thinking || *kind == DeltaKind::Thinking,
                        text || *kind == DeltaKind::Text,
                    ));
                }
            }
        })
    });
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("timeline", window, cx))
        .expect("started");
    wait_until(cx, "the timeline turn's result", |cx| {
        one.view.read_with(cx, |v, _| {
            v.items
                .iter()
                .filter(|i| matches!(i, Item::Done { .. }))
                .count()
                == 2
        })
    });
    assert_eq!(
        streamed.get(),
        (true, true),
        "thinking and text showed while streaming"
    );
    let id = one
        .view
        .read_with(cx, |v, _| v.thread.as_ref().map(|t| t.id))
        .expect("row");
    let events: Vec<Event> = one
        .store
        .lock()
        .expect("lock")
        .events_since(id, 0)
        .expect("events")
        .iter()
        .filter_map(|e| match Record::from_row(&e.kind, &e.payload_json) {
            Ok(Record::Agent(e)) => Some(e),
            _ => None,
        })
        .collect();
    assert!(
        !events.iter().any(|e| matches!(e, Event::Delta { .. })),
        "a delta was stored"
    );
    assert!(events.iter().any(|e| matches!(e, Event::Assistant { content }
        if matches!(&content[..], [Block::Text { text }] if text == "I'll add the parser, then test it."))));
    one.view.read_with(cx, |v, _| {
        assert!(
            !v.items.iter().any(|i| matches!(i, Item::Live { .. })),
            "the complete blocks replaced the streaming rows"
        );
    });
}
