//! Find in thread: mod+F opens the bar over the timeline, typing counts every match (case-insensitive), Enter and
//! Shift-Enter step through them, a match in a folded turn opens the fold and its row, and Esc closes the bar.

#[cfg(unix)]
use super::claude_folded_turns_test::one_view;
use super::thread_find::matches;
use super::thread_model::{Item, ToolItem};
#[cfg(unix)]
use super::timeline_fold::fold_key;
#[cfg(unix)]
use gpui::TestAppContext;
#[cfg(unix)]
use skyelab_core::Provider;
#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
const M: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};

/// A finished turn: a tool that mentions the needle (folded), then a reply that says it twice.
fn turn() -> Vec<Item> {
    vec![
        Item::Prompt("find it".into()),
        Item::Tool(ToolItem {
            id: "t1".into(),
            name: "Bash".into(),
            input: "{\"command\": \"grep needle\"}".into(),
            raw: serde_json::json!({"command": "grep needle"}),
            result: Some("found".into()),
            is_error: false,
        }),
        Item::Text("The Needle is here, needle.".into()),
        Item::Done {
            is_error: false,
            duration_ms: 1,
            cost_usd: 0.,
            usage: None,
            at_ms: Some(1),
            error: None,
        },
    ]
}

#[test]
fn matches_count_each_occurrence() {
    let items = turn();
    assert_eq!(matches(&items, "NEEDLE"), [1, 2, 2]);
    assert_eq!(matches(&items, "found"), [1], "a tool's result is searched");
    assert!(matches(&items, "  ").is_empty());
    assert!(matches(&items, "absent").is_empty());
}

#[cfg(unix)] // the fake agent is a shell script
#[gpui::test]
fn find_keys_open_step_and_close(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    one.view.update_in(cx, |v, window, cx| {
        v.items = turn();
        v.list.reset(v.items.len());
        v.focus_prompt(window, cx);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        one.view.read_with(cx, |v, _| !v.is_expanded(&fold_key(3))),
        "the turn starts folded"
    );

    // From the focused prompt, mod+F opens the bar.
    cx.simulate_keystrokes(&format!("{M}-f"));
    cx.run_until_parked();
    assert!(cx.debug_bounds("find-bar").is_some());
    cx.simulate_keystrokes("n e e d l e");
    cx.run_until_parked();
    let state = |cx: &mut gpui::VisualTestContext| {
        one.view.read_with(cx, |v, _| {
            let f = &v.timeline.find;
            (f.matches.clone(), f.active)
        })
    };
    assert_eq!(state(cx), (vec![1, 2, 2], 0));
    one.view.read_with(cx, |v, _| {
        assert!(v.is_expanded(&fold_key(3)), "the fold opened for the match");
        assert!(v.is_expanded("t1"), "and the tool row");
        assert_eq!(v.timeline.find.count("needle"), "1/3");
        assert_eq!(v.timeline.find.mark(1), Some(true));
        assert_eq!(v.timeline.find.mark(2), Some(false));
        assert_eq!(v.timeline.find.mark(0), None);
    });
    assert!(
        cx.debug_bounds("find-active").is_some(),
        "the current match is tinted"
    );

    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(state(cx).1, 1);
    cx.simulate_keystrokes("enter enter");
    cx.run_until_parked();
    assert_eq!(state(cx).1, 0, "wraps around");
    cx.simulate_keystrokes("shift-enter");
    cx.run_until_parked();
    assert_eq!(state(cx).1, 2, "backwards");

    // A reply streams in with another match: counted, and the place (row 2) is kept.
    one.view.update(cx, |v, cx| {
        v.items.push(Item::Text("one more needle".into()));
        v.list.reset(v.items.len());
        super::thread_find::FindHost::find_rows_changed(v);
        cx.notify();
    });
    assert_eq!(state(cx), (vec![1, 2, 2, 4], 1));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(cx.debug_bounds("find-bar").is_none());
    assert!(
        one.view
            .read_with(cx, |v, _| v.timeline.find.input.is_none())
    );
    assert!(
        cx.debug_bounds("find-active").is_none(),
        "no tint once closed"
    );
}
