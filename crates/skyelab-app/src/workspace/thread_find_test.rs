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
            total_usd: 0.,
            usage: None,
            at_ms: Some(1),
            error: None,
            copied: false,
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

#[test]
fn word_ranges_find_each_match_in_the_text() {
    use super::find_marks::word_ranges;
    assert_eq!(word_ranges("The Needle, needle.", "needle"), [4..10, 12..18]);
    assert_eq!(word_ranges("ÉCOLE école", "école"), [0..6, 7..13], "non-ASCII case");
    // `İ` lowercases to two chars: the range still ends on a char boundary of the text.
    let text = "İx ix";
    for r in word_ranges(text, "x") {
        assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
    }
    assert!(word_ranges("abc", " ").is_empty());
}

#[test]
fn a_reply_is_counted_as_drawn() {
    use skyelab_agent::DeltaKind;
    let reply = |t: &str| vec![Item::Text(t.into())];
    assert!(
        matches(&reply("see [docs](a.rs)"), "a.rs").is_empty(),
        "a link target isn't drawn, so it isn't counted"
    );
    assert_eq!(matches(&reply("see [a.rs](a.rs)"), "a.rs"), [0], "its text is, once");
    assert_eq!(matches(&reply("**bold** move"), "bold move"), [0], "markup left out");
    assert_eq!(matches(&reply(r"a\_b"), "a_b"), [0], "an escape resolved");
    let live = vec![Item::Live {
        kind: DeltaKind::Text,
        text: "[x](needle.rs) needle".into(),
    }];
    assert_eq!(matches(&live, "needle"), [0], "a streaming reply part too");
}

#[test]
fn the_greek_final_sigma_matches_either_way() {
    let items = vec![Item::Prompt("ΟΔΟΣ οδος".into())];
    assert_eq!(matches(&items, "οδος"), [0, 0]);
    assert_eq!(matches(&items, "ΟΔΟΣ"), [0, 0]);
    assert_eq!(
        super::find_marks::word_ranges("ΟΔΟΣ οδος", "οδοσ").len(),
        2,
        "and marks both"
    );
}

/// The count is the number of marks: each reply's matches in gpui-component's own rendered text.
#[gpui::test]
fn the_count_equals_the_marks_in_the_rendered_text(cx: &mut gpui::TestAppContext) {
    use gpui::AppContext as _;
    cx.update(gpui_component::init);
    for (source, query) in [
        ("see [docs](needle.rs) and needle", "needle"),
        ("# Needle\n\n- a needle\n- `needle`\n\n```\nneedle\n```", "needle"),
        ("| a | needle |\n|---|---|\n| needle | b |", "needle"),
        ("**bold** text, _it_ here", "bold text"),
        (r"a\_b and a_b", "a_b"),
        ("end of one.\n\nStart of two", "one. start"),
        ("the needle\nhaystack here", "needle haystack"),
        ("two  spaces", "two spaces"),
        ("| a | b |\n|---|---|\n| c | d |", "b c"),
        ("| a | b |\n|---|---|\n| c | d |", "b\nc"),
        ("a <strong>bold</strong> word, a <Button> tag", "strong"),
        ("a <strong>bold</strong> word", "bold word"),
        ("fix the <Button> now", "button"),
        ("fix the <Button> now", "the now"),
    ] {
        let state = cx.new(|cx| gpui_component::text::TextViewState::markdown(source, cx));
        cx.run_until_parked();
        let rendered = state.read_with(cx, |s, _| s.rendered_text().as_str().to_owned());
        assert_eq!(
            matches(&[Item::Text(source.into())], query).len(),
            super::find_marks::word_ranges(&rendered, query).len(),
            "{source:?} as {rendered:?}"
        );
    }
}

#[cfg(unix)] // the fake agent is a shell script
#[gpui::test]
fn find_keys_open_step_and_close(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    one.view.update_in(cx, |v, window, cx| {
        v.items = turn().into();
        v.lists.reset(v.items.len());
        v.focus_prompt(window, cx);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        one.view
            .read_with(cx, |v, _| !v.is_expanded(&fold_key(&v.items, 3))),
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
        assert!(
            v.is_expanded(&fold_key(&v.items, 3)),
            "the fold opened for the match"
        );
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
    one.view.read_with(cx, |v, _| {
        let f = &v.timeline.find;
        assert_eq!(f.words(2), Some(("needle", Some(0))), "the row's first match is current");
        assert_eq!(f.words(1), Some(("needle", None)));
        assert_eq!(f.words(0), None);
    });
    cx.simulate_keystrokes("enter enter");
    cx.run_until_parked();
    assert_eq!(state(cx).1, 0, "wraps around");
    cx.simulate_keystrokes("shift-enter");
    cx.run_until_parked();
    assert_eq!(state(cx).1, 2, "backwards");

    // A reply streams in with another match: counted, and the place (row 2) is kept.
    one.view.update(cx, |v, cx| {
        v.items.push(Item::Text("one more needle".into()));
        v.lists.reset(v.items.len());
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
