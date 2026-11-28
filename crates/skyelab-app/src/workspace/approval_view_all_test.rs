//! Own UI kit, stage 7a: an approval's View all opens SkyeLab's own `DiffViewer` on the whole change.

use super::claude_folded_turns_test::{One, one_view, wait_until};
use super::thread_model::Item;
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use skyelab_core::Provider;
use std::path::Path;

/// Sends `prompt` to the fake agent, waits for its request and clicks View all.
fn view_all<'a>(prompt: &str, cx: &'a mut TestAppContext) -> (One, &'a mut VisualTestContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    one.view
        .update_in(cx, |v, window, cx| {
            v.mode = skyelab_agent::Mode::Default; // a new thread's Full access would skip the ask
            v.submit_remote(prompt, window, cx)
        })
        .expect("started");
    wait_until(cx, "the request in the thread", |cx| {
        one.view.read_with(cx, |v, _| {
            v.items.iter().any(|i| matches!(i, Item::Approval(_)))
        })
    });
    cx.run_until_parked();
    let all = cx.debug_bounds("approval-view-all").expect("View all");
    cx.simulate_click(all.center(), Modifiers::none());
    cx.run_until_parked();
    cx.debug_bounds("diff-viewer-code-0")
        .expect("our viewer's first line");
    (one, cx)
}

#[gpui::test]
fn view_all_shows_the_last_line_of_a_big_write_and_escape_closes_it(cx: &mut TestAppContext) {
    let (_one, cx) = view_all("permit write", cx);
    // No click: the keyboard alone reads the whole change.
    cx.simulate_keystrokes("end");
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("diff-viewer-code-1999").is_some(),
        "line 2,000 can be read before it is allowed"
    );
    cx.simulate_keystrokes("escape");
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(1));
    cx.run_until_parked();
    assert!(
        !cx.update(crate::ui::has_active_dialog),
        "Escape closes the dialog"
    );
}

#[gpui::test]
fn view_all_divides_a_multi_edit_where_its_second_edit_starts(cx: &mut TestAppContext) {
    // Two edits: one line for one (2 rows), then twelve for twelve (24 rows), past the card's 20.
    let (_one, cx) = view_all("permit big multi", cx);
    assert!(
        cx.debug_bounds("diff-viewer-break-2").is_some(),
        "a divider before the second edit"
    );
    cx.simulate_keystrokes("end");
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("diff-viewer-code-25").is_some(),
        "all 26 rows are there"
    );
}
