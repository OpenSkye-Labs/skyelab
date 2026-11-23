//! The Details tab redraws when what its card shows of the thread changes, not on every notify of the thread view
//! (a streamed delta, a keystroke).

use super::Workspace;
use super::layout::ToggleRightPanel;
use super::thread_model::Item;
use super::threads_test::{
    fixture, open_workspace, project_thread, setup, type_and_send, wait_done,
};
use gpui::{Entity, TestAppContext, VisualTestContext};
use skyelab_agent::DeltaKind;

fn renders(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> usize {
    ws.read_with(cx, |ws, cx| ws.details.read(cx).renders)
}

#[gpui::test]
fn a_streamed_delta_doesnt_redraw_the_details_tab(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    cx.dispatch_action(ToggleRightPanel);
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("thread-details-card").is_some(),
        "the tab is drawn"
    );
    let before = renders(&ws, cx);
    thread.update(cx, |t, cx| {
        t.items.push(Item::Live {
            kind: DeltaKind::Text,
            text: "streamed".into(),
        });
        t.lists.splice(t.items.len() - 1..t.items.len() - 1, 1);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("streaming-text").is_some(),
        "the delta is drawn in the timeline"
    );
    assert_eq!(renders(&ws, cx), before, "DETAILS_REDRAWN_ON_DELTA");
    // Something the card shows still redraws it.
    thread.update(cx, |t, cx| {
        t.children.push(super::thread_details::ChildLink {
            id: 99,
            title: "child".into(),
            status: crate::ui::Status::Running,
            started_ms: 1,
        });
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        renders(&ws, cx) > before,
        "a change the card shows redraws it"
    );
}
