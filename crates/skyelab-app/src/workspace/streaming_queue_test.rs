//! Streaming text meets the composer's queue and steer. A plain Send while text streams waits in the queue (no row
//! mid-stream) and runs as the next turn; a steered message lands in the running turn, and the reply still shows
//! once.

use super::claude_folded_turns_test::{One, one_view, wait_until};
use super::thread_model::Item;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::DeltaKind;
use skyelab_core::Provider;
use std::path::Path;

const REPLY: &str = "I'll add the parser, then test it.";

fn streaming(cx: &mut TestAppContext) -> (One, &mut VisualTestContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    // The fake's opening replay has no result, so the turn runs until the streamed one ends.
    let (one, cx) = one_view(cx, Provider::Claude, bin, Some(".fake-agent-no-result"));
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("timeline", window, cx))
        .expect("started");
    wait_until(cx, "the reply streaming", |cx| {
        one.view.read_with(cx, |v, _| {
            v.items.iter().any(|i| {
                matches!(
                    i,
                    Item::Live {
                        kind: DeltaKind::Text,
                        ..
                    }
                )
            })
        })
    });
    (one, cx)
}

fn type_and(one: &One, text: &str, steer: bool, cx: &mut VisualTestContext) {
    one.view.update_in(cx, |v, window, cx| {
        v.prompt.update(cx, |p, cx| p.set_value(text, window, cx));
        if steer {
            v.steer_prompt(&super::thread::composer::SteerPrompt, window, cx);
        } else {
            v.send_prompt(&super::thread::SendPrompt, window, cx);
        }
    });
}

fn replies(one: &One, cx: &mut VisualTestContext) -> usize {
    one.view.read_with(cx, |v, _| {
        v.items
            .iter()
            .filter(|i| match i {
                Item::Text(t) => REPLY.starts_with(t.as_str()) || t == REPLY,
                Item::Live { text, .. } => REPLY.starts_with(text.as_str()),
                _ => false,
            })
            .count()
    })
}

fn prompts(one: &One, text: &str, cx: &mut VisualTestContext) -> Option<usize> {
    one.view.read_with(cx, |v, _| {
        v.items
            .iter()
            .position(|i| matches!(i, Item::Prompt(p) if p == text))
    })
}

fn done_at(one: &One, cx: &mut VisualTestContext) -> Option<usize> {
    one.view.read_with(cx, |v, _| {
        v.items.iter().rposition(|i| matches!(i, Item::Done { .. }))
    })
}

#[gpui::test]
fn a_steer_mid_stream_keeps_one_reply(cx: &mut TestAppContext) {
    let (one, cx) = streaming(cx);
    type_and(&one, "and also this", true, cx);
    wait_until(cx, "the steered prompt in the thread", |cx| {
        prompts(&one, "and also this", cx).is_some()
    });
    wait_until(cx, "the turn's result", |cx| {
        done_at(&one, cx).is_some_and(|d| d > prompts(&one, "and also this", cx).unwrap_or(0))
    });
    assert_eq!(replies(&one, cx), 1, "the streamed reply shows once");
}

#[gpui::test]
fn a_send_mid_stream_waits_for_the_turn(cx: &mut TestAppContext) {
    let (one, cx) = streaming(cx);
    type_and(&one, "next please", false, cx);
    cx.run_until_parked();
    assert_eq!(
        prompts(&one, "next please", cx),
        None,
        "queued, not in the thread yet"
    );
    wait_until(cx, "the queued prompt sent after the turn", |cx| {
        prompts(&one, "next please", cx).is_some()
    });
    let done = done_at(&one, cx).expect("the first turn ended");
    assert!(
        prompts(&one, "next please", cx).is_some_and(|p| p > done),
        "the queued prompt runs as the next turn"
    );
    assert_eq!(replies(&one, cx), 1, "the streamed reply shows once");
}
