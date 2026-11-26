//! The edit card's diff draws only the rows in view: a 2,000-line Write builds a card's worth per frame, Show all
//! lifts the height, and a folded unchanged run opens on click.

use super::claude_folded_turns_test::one_view;
use super::diff_view::{MAX_ROWS, rows_built};
use super::thread_model::{Item, ToolItem};
use gpui::{Modifiers, TestAppContext};
use serde_json::json;
use skyelab_core::Provider;
use std::path::Path;

fn tool(id: &str, name: &str, raw: serde_json::Value) -> Item {
    Item::Tool(ToolItem {
        id: id.into(),
        name: name.into(),
        input: raw.to_string(),
        raw,
        result: Some("ok".into()),
        is_error: false,
    })
}

#[gpui::test]
fn a_long_write_builds_only_the_rows_in_view_and_runs_fold(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    let content: String = (0..2_000).map(|i| format!("x_{i} = {i}\n")).collect();
    let same: String = (0..20).map(|i| format!("line {i}\n")).collect();
    let items = vec![
        tool("w1", "Write", json!({ "file_path": "/w/big.py", "content": content })),
        tool(
            "e1",
            "Edit",
            json!({ "file_path": "/w/a.py", "old_string": format!("{same}old"), "new_string": format!("{same}new") }),
        ),
    ];
    let before = rows_built();
    one.view.update(cx, |v, cx| {
        v.items = items.into();
        v.lists.reset(v.items.len());
        cx.notify();
    });
    cx.run_until_parked();
    assert!(rows_built() > before && cx.debug_bounds("edit-card").is_some());
    // One more frame: two cards of at most MAX_ROWS each (a list may lay a row out twice), never the 400 a Write
    // card holds.
    let before = rows_built();
    one.view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let built = rows_built() - before;
    assert!(built > 0 && built <= 4 * MAX_ROWS, "built {built} rows in one frame");

    // The Edit's 20 shared lines fold above the change; a click opens them.
    let gap = cx.debug_bounds("diff-gap").expect("a folded run is drawn");
    cx.simulate_click(gap.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(
        one.view.read_with(cx, |v, _| v.is_expanded("e1:gap:0")),
        "the click opened the run"
    );

    // Both cards are past MAX_ROWS now (the opened Edit too), so either one's Show all may be the one found.
    let all = cx.debug_bounds("edit-show-all").expect("a long card offers Show all");
    cx.simulate_click(all.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(one.view.read_with(cx, |v, _| v.is_expanded("w1:all") || v.is_expanded("e1:all")));
}
