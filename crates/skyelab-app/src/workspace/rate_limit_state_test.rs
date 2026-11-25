//! A usage limit shows as Limited in the sidebar, as the timeline's card and the limit banner over the composer; a turn
//! that succeeds afterwards clears it.

use super::claude_folded_turns_test::{one_view, wait_until};
use super::thread_model::Item;
use crate::ui::Status;
use gpui::TestAppContext;
use skyelab_agent::Decision;
use skyelab_core::Provider;
use std::path::Path;

#[gpui::test]
fn a_limit_shows_until_a_turn_succeeds(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("timeline", window, cx))
        .expect("started");
    wait_until(cx, "the limit", |cx| {
        one.view.read_with(cx, |v, _| v.limited.is_some())
    });
    assert_eq!(
        one.view.read_with(cx, |v, _| v.live_status()),
        Status::Limited
    );
    cx.run_until_parked();
    assert!(cx.debug_bounds("limit-banner").is_some(), "the limit banner docks");
    // A turn that succeeds: ask, answer, result.
    one.view
        .update_in(cx, |v, window, cx| {
            v.submit_remote("question again", window, cx)
        })
        .expect("sent");
    let ask = |cx: &mut gpui::VisualTestContext| {
        one.view.read_with(cx, |v, _| {
            v.items.iter().find_map(|i| match i {
                Item::Approval(a) if a.id.ends_with("q-1") && a.decision.is_none() => {
                    Some(a.id.clone())
                }
                _ => None,
            })
        })
    };
    wait_until(cx, "the question", |cx| ask(cx).is_some());
    let id = ask(cx).expect("asked");
    one.view
        .update(cx, |v, cx| v.answer(&id, Decision::Deny, cx));
    wait_until(cx, "the limit to clear", |cx| {
        one.view.read_with(cx, |v, _| v.limited.is_none())
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("limit-banner").is_none(), "gone with the limit");
    assert_ne!(
        one.view.read_with(cx, |v, _| v.live_status()),
        Status::Limited
    );
}
