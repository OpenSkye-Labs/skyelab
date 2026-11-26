//! A waiting question, the usage-limit banner and the composer stay compact (T3's one docked panel): in an 840 px
//! window they never take half of it, and the plan's Approve above them stays in view (QA: they took 420 px and
//! Approve scrolled under the banner).

use super::claude_folded_turns_test::{one_view, wait_until};
use gpui::{TestAppContext, px, size};
use skyelab_core::Provider;
use std::path::Path;

#[gpui::test]
fn pending_cards_leave_the_plan_in_view(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    cx.simulate_resize(size(px(560.), px(840.)));
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("timeline", window, cx))
        .expect("started");
    wait_until(cx, "the question docked, after the plan and the limit", |cx| {
        one.view
            .read_with(cx, |v, _| v.limited.is_some() && v.dock_step().is_some())
    });
    cx.run_until_parked();
    let banner = cx.debug_bounds("limit-banner").expect("the limit banner");
    let dock = cx.debug_bounds("ask-dock").expect("the question panel");
    let approve = cx.debug_bounds("plan-actions").expect("the plan's buttons");
    let stack_top = banner.top().min(dock.top());
    assert!(
        banner.size.height <= px(36.),
        "the banner is one line: {:?}",
        banner.size.height
    );
    assert!(
        px(840.) - stack_top <= px(340.),
        "the pending stack and composer take {:?} of 840",
        px(840.) - stack_top
    );
    assert!(
        approve.bottom() <= stack_top,
        "Approve ({approve:?}) is above the stack (top {stack_top:?})"
    );
}
