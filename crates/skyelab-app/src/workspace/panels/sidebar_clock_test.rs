//! Acceptance: an idle sidebar's "now" turns into "1m" a minute later without the rows being reloaded; a minute that
//! changes no label draws nothing, and neither does an inactive window until it is active again.

use super::super::{Sidebar, SidebarItem, SidebarSection};
use gpui::{Entity, TestAppContext, VisualTestContext};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

const T0: i64 = 1_800_000_000_000;
static NOW: AtomicI64 = AtomicI64::new(T0);

fn now() -> i64 {
    NOW.load(Ordering::SeqCst)
}

fn sidebar(cx: &mut TestAppContext) -> (Entity<Sidebar>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
    });
    let item = SidebarItem {
        id: "thread-1".into(),
        label: "Fix it".into(),
        selected: false,
        action: Box::new(crate::workspace::NewThread),
        status: None,
        updated_ms: Some(T0 - 30_000),
        account: None,
        project: None,
    };
    let section = SidebarSection {
        title: "Threads".into(),
        items: vec![item],
        empty_title: "".into(),
        empty_hint: "".into(),
        button: None,
        foldable: false,
    };
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut s = Sidebar::new(vec![section], window, cx);
        s.clock.now = now;
        s.rows_changed(cx);
        s
    });
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    (view, cx)
}

fn minute(cx: &mut VisualTestContext) -> u64 {
    crate::debug_renders::take_test_count("Sidebar");
    cx.executor().advance_clock(Duration::from_secs(60));
    cx.run_until_parked();
    crate::debug_renders::take_test_count("Sidebar")
}

fn label(view: &Entity<Sidebar>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |s, _| s.time_labels(now()).concat())
}

#[gpui::test]
fn row_times_move_on_without_a_reload(cx: &mut TestAppContext) {
    NOW.store(T0, Ordering::SeqCst);
    let (view, cx) = sidebar(cx);
    let rebuilds = view.read_with(cx, |s, _| s.rebuilds);
    assert_eq!(label(&view, cx), "now");

    // A minute whose labels read the same draws nothing.
    NOW.store(T0 + 10_000, Ordering::SeqCst);
    assert_eq!(minute(cx), 0, "REDRAW_FOR_AN_UNCHANGED_LABEL");

    // A minute later the row reads "1m", drawn without the rows being rebuilt.
    NOW.store(T0 + 40_000, Ordering::SeqCst);
    assert!(minute(cx) > 0, "TIME_DIDNT_REDRAW");
    assert_eq!(label(&view, cx), "1m");
    assert_eq!(
        view.read_with(cx, |s, _| s.rebuilds),
        rebuilds,
        "the rows weren't reloaded"
    );

    // Inactive: no redraw even though "2m" is due; active again, it catches up at once.
    cx.deactivate_window();
    NOW.store(T0 + 100_000, Ordering::SeqCst);
    assert_eq!(minute(cx), 0, "REDRAW_WHILE_INACTIVE");
    crate::debug_renders::take_test_count("Sidebar");
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    assert!(
        crate::debug_renders::take_test_count("Sidebar") > 0,
        "ACTIVATION_DIDNT_CATCH_UP"
    );
}
