//! Acceptance: wheel scrolling a long thread moves its content by exactly the wheel's distance, every step. Rows that
//! finish laying out late (replies over 4 KB parse in the background) must not make the view jump.

use super::thread_model::Record;
use super::*;
use gpui::{
    Modifiers, ScrollDelta, ScrollEvent, TestAppContext, TouchPhase, VisualTestContext, point,
    px,
};

/// A reply of about `kb` KB of markdown: headings, paragraphs and a list, so it lays out tall.
fn reply(n: usize, kb: usize) -> String {
    let para = "The quick brown fox jumps over the lazy dog, and then it reads the whole log again. ";
    let mut s = format!("## Reply {n}\n\n");
    while s.len() < kb * 1024 {
        s.push_str(&para.repeat(4));
        s.push_str("\n\n- one item\n- another item\n\n");
    }
    s
}

fn long_thread(cx: &mut TestAppContext) -> (Entity<ThreadView>, &mut VisualTestContext) {
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
    for n in 0..40 {
        let (kind, payload) = Record::Prompt(format!("question {n}")).to_row();
        store.append_event(thread.id, kind, &payload).expect("prompt");
        // Every other reply is over the 4 KB a TextView parses on the UI thread.
        let text = reply(n, if n % 2 == 0 { 9 } else { 1 });
        let payload = serde_json::json!({"Assistant": {"content": [{"type": "text", "text": text}]}});
        store
            .append_event(thread.id, "agent", &payload.to_string())
            .expect("reply");
    }
    let backend = Backend {
        store: Arc::new(Mutex::new(store)),
        claude_bin: Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh"),
    };
    cx.add_window_view(|window, cx| {
        ThreadView::open(thread, AccountsConfig::default(), backend, window, cx)
    })
}

/// One wheel step of `dy` pixels (positive scrolls up) over the timeline.
fn wheel(dy: f32, cx: &mut VisualTestContext) {
    cx.simulate_event(ScrollEvent {
        position: point(px(300.), px(200.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
        source: Default::default(),
    });
    cx.run_until_parked();
}

/// (row, its top on screen) for the first row drawn in the viewport.
fn anchor(view: &Entity<ThreadView>, cx: &mut VisualTestContext) -> (usize, gpui::Pixels) {
    view.read_with(cx, |v, _| {
        let list = v.own_list();
        let ix = list.logical_scroll_top().item_ix;
        (ix, list.bounds_for_item(ix).expect("drawn").top())
    })
}

/// (row, its top on screen) for the tallest row drawn near the top: it stays drawn through one wheel step.
fn reference(view: &Entity<ThreadView>, cx: &mut VisualTestContext) -> (usize, gpui::Pixels) {
    view.read_with(cx, |v, _| {
        let list = v.own_list();
        let top = list.logical_scroll_top().item_ix;
        (top..top + 10)
            .filter_map(|ix| list.bounds_for_item(ix).map(|b| (ix, b)))
            .max_by(|a, b| f32::from(a.1.size.height).total_cmp(&f32::from(b.1.size.height)))
            .map(|(ix, b)| (ix, b.top()))
            .expect("a drawn row")
    })
}

#[gpui::test]
fn wheel_steps_up_a_long_thread_move_it_by_the_wheel_distance(cx: &mut TestAppContext) {
    let (view, cx) = long_thread(cx);
    cx.run_until_parked();
    let rows = view.read_with(cx, |v, _| v.own_list().item_count());
    assert!(rows >= 80, "{rows} rows");
    let mut jumps = Vec::new();
    steps(&view, 40., 400, &mut jumps, cx);
    // And back down, as far.
    steps(&view, -40., 400, &mut jumps, cx);
    assert!(jumps.is_empty(), "{} jumps:\n{}", jumps.len(), jumps.join("\n"));
}

/// `n` wheel steps of `dy`, noting each one that didn't move the content by `dy`.
fn steps(
    view: &Entity<ThreadView>,
    dy: f32,
    n: usize,
    jumps: &mut Vec<String>,
    cx: &mut VisualTestContext,
) {
    for step in 0..n {
        let (ix, top) = reference(view, cx);
        if ix == 0 && view.read_with(cx, |v, _| v.own_list().logical_scroll_top().offset_in_item) == px(0.) {
            break; // at the top
        }
        wheel(dy, cx);
        let after = view.read_with(cx, |v, _| v.own_list().bounds_for_item(ix).map(|b| b.top()));
        let Some(after) = after else {
            jumps.push(format!("step {step}: row {ix} no longer drawn"));
            continue;
        };
        let moved = after - top;
        if (moved - px(dy)).abs() > px(1.) {
            jumps.push(format!("step {step}: row {ix} moved {moved:?}, not {dy}px"));
        }
    }
}

/// A row changed in place (a tool result landing, a fold toggled) keeps the reader where they were inside it.
#[gpui::test]
fn a_row_changed_in_place_keeps_the_reading_offset(cx: &mut TestAppContext) {
    let (view, cx) = long_thread(cx);
    cx.run_until_parked();
    let row = view.read_with(cx, |v, _| v.own_list().item_count()) - 21; // a 9 KB reply
    view.update(cx, |v, cx| {
        v.own_list().scroll_to(gpui::ListOffset {
            item_ix: row,
            offset_in_item: px(300.),
        });
        cx.notify();
    });
    cx.run_until_parked();
    let before = anchor(&view, cx);
    view.update(cx, |v, cx| {
        v.lists.splice(row..row + 1, 1);
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(anchor(&view, cx), before, "not back at the row's top");
    let offset = view.read_with(cx, |v, _| v.own_list().logical_scroll_top().offset_in_item);
    assert_eq!(offset, px(300.));
}
