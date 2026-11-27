//! Criterion 13: a child started by delegation shows "Delegated by <title> on <machine>" in its header here, and
//! the parent on the other machine lists it with "on <machine>".

use super::remote_delegation_a_harness as a;
use super::remote_delegation_harness::*;
use super::thread_details::remote::remote_label;
use crate::test_wait::wait_until;
use gpui::TestAppContext;

#[gpui::test]
fn the_childs_header_names_the_parent_and_its_machine(cx: &mut TestAppContext) {
    let (b, cx) = b_side(
        Setup {
            link_to: Some("work"),
            ..Setup::default()
        },
        cx,
    );
    let id = started(&b, &caller(), start_op(b.project("work-a"), "r1"), cx);
    let view = b.ws.read_with(cx, |ws, cx| ws.open_view(id, cx)).expect("its tab");
    let label = view.read_with(cx, |v, _| v.delegated_label()).expect("a label");
    assert_eq!(label.to_string(), "Delegated by parent on phone");
    // Reopened from the store (after a restart), it still says so.
    let (config, backend) = b.ws.read_with(cx, |ws, _| (ws.config.clone(), ws.backend.clone()));
    let row = b.store.lock().expect("lock").thread(id).expect("read").expect("row");
    let (reopened, cx) = cx.add_window_view(|window, cx| {
        super::thread::ThreadView::open(row, config, backend, window, cx)
    });
    wait_until(cx, "the reopened view's origin", |cx| {
        reopened.read_with(cx, |v, _| v.delegated_label().is_some())
    });
    // And its header draws it.
    cx.run_until_parked();
    assert!(cx.debug_bounds("thread-header-delegated").is_some(), "the header's origin is drawn");
    let snaps = cx.update(|window, _| gpui_base::test_support::snapshots(window));
    let texts: Vec<_> = snaps.iter().filter_map(|s| s.label()).collect();
    assert!(texts.contains(&"Delegated by parent on phone"), "{texts:?}");
}

#[gpui::test]
fn the_parent_lists_its_remote_child_with_the_machine(cx: &mut TestAppContext) {
    let (side, cx) = a::a_side(true, cx);
    let started = a::on_b(&side, a::start(1), cx).expect("started");
    assert_eq!(started["thread_id"], 900);
    let parent = side.ws.read_with(cx, |ws, cx| ws.open_view(side.parent, cx)).expect("parent tab");
    wait_until(cx, "the parent's remote row", |cx| {
        parent.read_with(cx, |v, _| !v.details.remote.is_empty())
    });
    let machine = parent.read_with(cx, |v, _| v.details.remote[0].machine.clone());
    assert_eq!(remote_label(&machine).to_string(), "on desktop");
}
