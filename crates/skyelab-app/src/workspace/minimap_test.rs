//! The timeline minimap: a tick per prompt, the current turn's tick, and a click that jumps to its prompt.

use super::*;
use crate::workspace::thread_model::Item;

#[test]
fn ticks_spread_down_the_rail_and_the_current_one_follows_the_top_row() {
    let items = [
        Item::Prompt("one".into()),
        Item::Text("a".into()),
        Item::Prompt("two".into()),
        Item::Steer("steered".into()),
        Item::Text("b".into()),
        Item::Prompt("three".into()),
    ];
    let rows = prompt_rows(&items);
    assert_eq!(rows, [0, 2, 5], "steers fold into their turn");
    assert_eq!(tick_at(0, 3), 0.0);
    assert_eq!(tick_at(1, 3), 0.5);
    assert_eq!(tick_at(2, 3), 1.0);
    assert_eq!(current(&rows, 0), Some(0));
    assert_eq!(current(&rows, 4), Some(1), "reading turn two's reply");
    assert_eq!(current(&rows, 9), Some(2));
    assert_eq!(reply(&items, 2), Some("b"));
    assert_eq!(reply(&items, 5), None);
}

#[test]
fn a_preview_is_one_line_and_cut() {
    assert_eq!(preview("a\n\n  b\tc"), "a b c");
    let long = "x".repeat(PREVIEW + 10);
    assert_eq!(preview(&long).chars().count(), PREVIEW + 1);
}

#[cfg(unix)] // the fake agent is a shell script
#[gpui::test]
fn the_rail_draws_a_tick_per_prompt_and_a_click_jumps_to_it(cx: &mut gpui::TestAppContext) {
    use crate::workspace::claude_folded_turns_test::one_view;
    let bin =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, skyelab_core::Provider::Claude, bin, None);
    let mut items = Vec::new();
    for n in 0..3 {
        items.push(Item::Prompt(format!("prompt {n}")));
        for k in 0..30 {
            items.push(Item::Text(format!("reply {n}.{k}")));
        }
    }
    one.view.update(cx, |v, cx| {
        v.items = items.into();
        v.lists.reset(v.items.len());
        cx.notify();
    });
    cx.run_until_parked();
    for tick in ["minimap-tick-0", "minimap-tick-1", "minimap-tick-2"] {
        assert!(cx.debug_bounds(tick).is_some(), "{tick}");
    }
    assert!(cx.debug_bounds("minimap-tick-3").is_none());

    let tick = cx.debug_bounds("minimap-tick-1").expect("tick 1");
    cx.simulate_mouse_move(tick.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("minimap-card").is_some(),
        "hover shows the card"
    );
    cx.simulate_click(tick.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let top = one
        .view
        .read_with(cx, |v, _| v.own_list().logical_scroll_top().item_ix);
    assert_eq!(top, 31, "prompt 1's row is at the top");
}
