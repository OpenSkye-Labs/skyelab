//! A usage limit is told by one card in the timeline, and by T3's limit banner docked over the composer (its reset and
//! the Resume at reset and Snooze until reset actions).

use super::claude_folded_turns_test::{one_view, wait_until};
use super::thread_model::Item;
use gpui::TestAppContext;
use skyelab_core::Provider;
use std::path::Path;

#[gpui::test]
fn a_limit_is_told_once(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("timeline", window, cx))
        .expect("started");
    wait_until(cx, "the limit", |cx| {
        one.view.read_with(cx, |v, _| v.limited.is_some())
    });
    cx.run_until_parked();
    let banners = one.view.read_with(cx, |v, _| {
        v.items
            .iter()
            .filter(|i| matches!(i, Item::Limit { .. }))
            .count()
    });
    assert_eq!(banners, 1, "one banner card in the timeline");
    assert!(cx.debug_bounds("limit-row").is_some(), "the banner draws");
    assert!(cx.debug_bounds("limit-banner").is_some(), "the docked banner");
    assert!(cx.debug_bounds("limit-resume").is_some(), "Resume at reset");
    assert!(cx.debug_bounds("limit-snooze").is_some(), "Snooze until reset");
}
