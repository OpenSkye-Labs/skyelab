//! A finished turn folds its work behind "Worked for …": which rows fold, where the header goes, what stays (the
//! reply, questions), that running and failed turns don't fold, and that the header opens and folds the rows.

use super::thread_model::{Item, Record, ToolItem, apply, stamp_done};
use super::timeline_fold::{fold_head, fold_key, folded_by, reply_of, rows_of};
use skyelab_agent::Event;

fn tool(id: &str) -> Item {
    Item::Tool(ToolItem {
        id: id.into(),
        name: "Bash".into(),
        input: "{}".into(),
        raw: serde_json::json!({}),
        result: Some("ok".into()),
        is_error: false,
    })
}

fn done(is_error: bool) -> Item {
    Item::Done {
        is_error,
        duration_ms: 4200,
        cost_usd: 0.0,
        total_usd: 0.0,
        usage: None,
        at_ms: Some(1),
        error: None,
        copied: false,
    }
}

/// Two turns: the first with work and a reply, the second running.
fn turns() -> crate::workspace::thread_model::Items {
    vec![
        Item::Prompt("go".into()),    // 0
        tool("a"),                    // 1
        Item::Text("looking".into()), // 2: text between tools folds too
        tool("b"),                    // 3
        Item::Text("done".into()),    // 4: the reply stays
        done(false),                  // 5
        Item::Prompt("again".into()), // 6
        tool("c"),                    // 7: running, no divider yet
    ]
    .into()
}

#[test]
fn a_finished_turn_folds_its_work_but_not_its_reply() {
    let items = turns();
    let folded: Vec<usize> = (0..items.len())
        .filter(|&ix| folded_by(&items, ix).is_some())
        .collect();
    assert_eq!(folded, [1, 2, 3]);
    assert_eq!(folded_by(&items, 1), Some(5));
    assert_eq!(
        fold_head(&items, 5),
        Some(1),
        "the header goes above the first work row"
    );
    assert_eq!(reply_of(&items, 4), Some(5));
    assert_eq!(
        reply_of(&items, 2),
        None,
        "text the turn went on after is not the reply"
    );
    assert_eq!(
        rows_of(&items, &fold_key(&items, 5)),
        Some(1..6),
        "a toggle re-measures the whole turn"
    );
    assert_eq!(rows_of(&items, "c"), Some(7..8));
}

#[test]
fn failed_and_text_only_turns_keep_their_divider() {
    let failed = vec![Item::Prompt("go".into()), tool("a"), done(true)];
    assert_eq!(folded_by(&failed, 1), None);
    assert_eq!(fold_head(&failed, 2), None);
    let plain = vec![
        Item::Prompt("hi".into()),
        Item::Text("hello".into()),
        done(false),
    ];
    assert_eq!(
        fold_head(&plain, 2),
        None,
        "nothing to fold: the divider stays"
    );
}

#[test]
fn the_end_of_a_turn_redraws_the_whole_turn_and_is_dated() {
    let mut items: crate::workspace::thread_model::Items = vec![
        Item::Prompt("go".into()),
        tool("a"),
        Item::Text("done".into()),
    ]
    .into();
    let changed = apply(
        &mut items,
        &Record::Agent(Event::Result {
            is_error: false,
            duration_ms: 10,
            total_cost_usd: 0.0,
            result: None,
            prompts: 1,
            usage: None,
            turn_cost_usd: None,
        }),
    );
    assert_eq!(changed, Some(1), "its rows now fold, so they all change");
    stamp_done(&mut items, 42);
    stamp_done(&mut items, 99);
    assert!(
        matches!(
            items.last(),
            Some(Item::Done {
                at_ms: Some(42),
                ..
            })
        ),
        "stamped once"
    );
}

#[cfg(unix)] // the thread runs on the fake agent, a shell script
#[gpui::test]
fn the_header_opens_and_folds_the_turn(cx: &mut gpui::TestAppContext) {
    use super::claude_folded_turns_test::one_view;
    use skyelab_core::Provider;
    let bin =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    one.view.update(cx, |v, cx| {
        v.items = vec![
            Item::Prompt("go".into()),
            Item::Thinking {
                id: "think".into(),
                text: "hmm".into(),
            },
            Item::Text("done".into()),
            done(false),
        ]
        .into();
        v.lists.reset(v.items.len());
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("turn-fold").is_some(),
        "the header is drawn"
    );
    assert!(cx.debug_bounds("message-copy").is_some(), "the reply stays");
    assert!(
        cx.debug_bounds("thinking-row").is_none(),
        "folded by default"
    );
    one.view
        .update(cx, |v, cx| v.toggle_tool(&fold_key(&v.items, 3), cx));
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("thinking-row").is_some(),
        "the header opened it"
    );
}
