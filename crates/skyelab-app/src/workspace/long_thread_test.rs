//! Acceptance: a 10,000-event thread opens and appends without re-laying out every row, in remote and local tabs,
//! and both draw rows through the one shared renderer (`thread_items::render_item`).

use super::remote_thread::RemoteThreadView;
use super::thread_items::rows_built;
use super::thread_model::Record;
use super::*;
use crate::environment::RemoteEnv;
use gpui::{ListOffset, TestAppContext, VisualTestContext, point, px, size};
use skyelab_daemon::testing::{NODE_NAME, ca};
use std::time::Instant;

const N: i64 = 10_000;

fn event(seq: i64) -> skyelab_client::ThreadEvent {
    let (kind, payload_json) = Record::Prompt(format!("message {seq}")).to_row();
    skyelab_client::ThreadEvent {
        thread_id: 1,
        seq,
        kind: kind.into(),
        payload_json,
        created_at: 0,
    }
}

/// Draws `view` once at 800×600 and returns how many rows the shared renderer built.
fn frame<V: Render>(view: &Entity<V>, cx: &mut VisualTestContext) -> usize {
    let before = rows_built();
    cx.draw(point(px(0.), px(0.)), size(px(800.), px(600.)), |_, _| {
        view.clone().into_any_element()
    });
    rows_built() - before
}

#[gpui::test]
fn remote_thread_with_10k_events_opens_and_appends_without_full_relayout(cx: &mut TestAppContext) {
    super::env_harness::setup(cx);
    let (view, cx) = cx.add_window_view(|window, cx| {
        // Nothing listens on this port; the view never needs the connection here.
        let client = skyelab_client::Client::connect(
            NODE_NAME,
            skyelab_client::testing::options(9, ca().root()),
        )
        .expect("client thread");
        let env = cx.new(|cx| RemoteEnv::new(NODE_NAME.into(), client, cx));
        RemoteThreadView::new(env, 1, Some((1, "long".into())), window, cx)
    });

    let t = Instant::now();
    view.update(cx, |v, cx| {
        for seq in 1..=N {
            v.on_event(&event(seq), cx);
        }
    });
    let open = frame(&view, cx);
    let open_time = t.elapsed();
    let (items, count) = view.read_with(cx, |v, _| (v.items.len(), v.list.item_count()));
    assert_eq!((items, count), (N as usize, N as usize));
    assert!(open > 0 && open < 100, "first frame built {open} rows");

    view.read_with(cx, |v, _| {
        v.list.scroll_to(ListOffset {
            item_ix: 5_000,
            offset_in_item: px(0.),
        })
    });
    frame(&view, cx);
    let t = Instant::now();
    view.update(cx, |v, cx| v.on_event(&event(N + 1), cx));
    let append = frame(&view, cx);
    let append_time = t.elapsed();
    view.read_with(cx, |v, _| {
        assert_eq!(v.list.item_count(), N as usize + 1);
        // A `reset` would drop the scroll anchor and every measured height.
        assert_eq!(v.list.logical_scroll_top().item_ix, 5_000);
        assert!(v.list.bounds_for_item(5_000).is_some());
    });
    assert!(append < 100, "append frame built {append} rows");
    eprintln!(
        "remote 10k: open+frame {open_time:?} ({open} rows), append+frame {append_time:?} ({append} rows)"
    );
}

#[gpui::test]
fn local_thread_with_10k_events_draws_through_the_shared_renderer(cx: &mut TestAppContext) {
    super::env_harness::setup(cx);
    let mut store = Store::open_in_memory().expect("store");
    let project = store
        .create_project("app", Path::new("/src/app"))
        .expect("project");
    let thread = store
        .create_thread(&skyelab_store::NewThread {
            project_id: project.id,
            title: "long",
            cwd: Path::new("/src/app"),
            account_id: "work",
            status: "idle",
        })
        .expect("thread");
    for seq in 1..=N {
        let (kind, payload) = Record::Prompt(format!("message {seq}")).to_row();
        store
            .append_event(thread.id, kind, &payload)
            .expect("append");
    }
    let backend = Backend {
        store: Arc::new(Mutex::new(store)),
        claude_bin: Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh"),
    };
    let (view, cx) = cx.add_window_view(|window, cx| {
        ThreadView::open(thread, AccountsConfig::default(), backend, window, cx)
    });
    let t = Instant::now();
    cx.run_until_parked();
    let open = frame(&view, cx);
    let open_time = t.elapsed();
    let count = view.read_with(cx, |v, _| (v.items.len(), v.list.item_count()));
    assert_eq!(count, (N as usize, N as usize));
    assert!(open > 0 && open < 100, "first frame built {open} rows");
    eprintln!("local 10k: load+frame {open_time:?} ({open} rows)");
}
