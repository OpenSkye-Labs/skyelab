//! Claude's `total_cost_usd` is the process's running total: three turns of one process show and store what each
//! turn added, and the Usage page's sum is the last total, not the sum of the totals.

use super::claude_folded_turns_test::{one_view, wait_until};
use super::thread_model::Item;
use gpui::TestAppContext;
use skyelab_core::Provider;
use std::path::Path;

#[gpui::test]
fn three_turns_of_one_process_cost_the_last_total(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    let costs = |cx: &mut gpui::VisualTestContext| {
        one.view.read_with(cx, |v, _| {
            v.items
                .iter()
                .filter_map(|i| match i {
                    Item::Done { cost_usd, .. } => Some(*cost_usd),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
    };
    // The replayed session ends at $0.0421; each "turn" adds $0.0125 to the running total.
    for want in [2, 3] {
        one.view
            .update_in(cx, |v, window, cx| v.submit_remote("turn", window, cx))
            .expect("sent");
        wait_until(cx, "the turn's result", |cx| costs(cx).len() == want);
    }
    let costs = costs(cx);
    let expected = [0.0421, 0.0125, 0.0125];
    assert!(
        costs
            .iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 1e-9),
        "{costs:?}"
    );
    let rows = one.store.lock().expect("lock").usage_days(0).expect("rows");
    let spent: f64 = rows.iter().map(|r| r.cost_usd).sum();
    assert!((spent - 0.0671).abs() < 1e-9, "{spent}");
}
