//! Another machine's thread keeps the reader's place inside a row when a fold is toggled, as a local thread does.

use super::remote_thread::RemoteThreadView;
use super::thread_items::ItemsView as _;
use super::thread_model::{Item, Record};
use super::timeline_fold::fold_key;
use super::*;
use crate::environment::RemoteEnv;
use gpui::{ListOffset, TestAppContext, VisualTestContext, point, px, size};
use skyelab_agent::{Block, Event};
use skyelab_daemon::testing::{NODE_NAME, ca};

fn event(seq: i64, record: Record) -> skyelab_client::ThreadEvent {
    let (kind, payload_json) = record.to_row();
    skyelab_client::ThreadEvent {
        thread_id: 1,
        seq,
        kind: kind.into(),
        payload_json,
        created_at: 0,
    }
}

fn frame<V: Render>(view: &Entity<V>, cx: &mut VisualTestContext) {
    cx.draw(point(px(0.), px(0.)), size(px(800.), px(600.)), |_, _| {
        view.clone().into_any_element()
    });
}

#[gpui::test]
fn a_remote_fold_toggle_keeps_the_reading_offset(cx: &mut TestAppContext) {
    super::env_harness::setup(cx);
    let (view, cx) = cx.add_window_view(|window, cx| {
        // Nothing listens on this port; the view never needs the connection here.
        let client = skyelab_client::Client::connect(
            NODE_NAME,
            skyelab_client::testing::options(9, ca().root()),
        )
        .expect("client thread");
        let env = cx.new(|cx| RemoteEnv::new(NODE_NAME.into(), client, None, cx));
        RemoteThreadView::new(env, 1, Some((1, "long".into())), window, cx)
    });
    // Under 4 KB, so it lays out at once: a jump here comes from the splice, not a late parse.
    let reply =
        "The quick brown fox jumps over the lazy dog, and then it reads the whole log again. ".repeat(40);
    let mut records = vec![
        Record::Prompt("go".into()),
        Record::Agent(Event::Assistant {
            content: vec![Block::ToolUse {
                id: "t1".into(),
                name: "Bash".into(),
                input: serde_json::json!({"command": "ls"}),
            }],
        }),
        Record::Agent(Event::Assistant {
            content: vec![Block::Text { text: reply }],
        }),
        Record::Agent(Event::Result {
            is_error: false,
            duration_ms: 1000,
            total_cost_usd: 0.0,
            result: None,
            prompts: 1,
            usage: None,
            turn_cost_usd: None,
        }),
    ];
    for n in 0..30 {
        records.push(Record::Prompt(format!("later {n}")));
    }
    view.update(cx, |v, cx| {
        for (i, r) in records.into_iter().enumerate() {
            v.on_event(&event(i as i64 + 1, r), cx);
        }
    });
    frame(&view, cx);
    let (reply_ix, key) = view.read_with(cx, |v, _| {
        let r = v
            .items
            .iter()
            .position(|i| matches!(i, Item::Text(_)))
            .expect("reply row");
        let d = v
            .items
            .iter()
            .position(|i| matches!(i, Item::Done { .. }))
            .expect("done row");
        (r, fold_key(&v.items, d))
    });
    view.read_with(cx, |v, _| {
        v.list.scroll_to(ListOffset {
            item_ix: reply_ix,
            offset_in_item: px(150.),
        })
    });
    frame(&view, cx);
    cx.run_until_parked();
    frame(&view, cx);
    let before = view.read_with(cx, |v, _| v.list.logical_scroll_top());
    assert_eq!(
        (before.item_ix, before.offset_in_item),
        (reply_ix, px(150.)),
        "precondition"
    );

    view.update(cx, |v, cx| v.toggle_tool(&key, cx));
    frame(&view, cx);
    let after = view.read_with(cx, |v, _| v.list.logical_scroll_top());
    assert_eq!(
        (after.item_ix, after.offset_in_item),
        (reply_ix, px(150.)),
        "remote fold toggle moved the reader off their place in the reply"
    );
}
