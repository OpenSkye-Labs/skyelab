//! Acceptance 13: a run missed while the app was closed runs once at the next tick, or is skipped per the setting.

use super::super::threads_test::wait_until;
use super::test_util::{HOUR, NOW, open, read, rig, runs, set_settings, task, tick};
use gpui::TestAppContext;
use skyelab_core::ScheduledSettings;

#[gpui::test]
fn a_missed_run_runs_once(cx: &mut TestAppContext) {
    let rig = rig(true);
    let t = task(&rig, "every 1h", NOW - 3 * HOUR);
    let (_ws, cx) = open(&rig, cx);
    tick(cx);
    wait_until(cx, "the missed run", |_| runs(&rig, t.id).len() == 1);
    tick(cx);
    cx.run_until_parked();
    assert_eq!(runs(&rig, t.id).len(), 1, "three missed slots, one run");
    assert_eq!(read(&rig.f.store, t.id).next_run_at, Some(NOW + HOUR));
}

#[gpui::test]
fn a_missed_run_is_skipped_when_set(cx: &mut TestAppContext) {
    let rig = rig(true);
    let t = task(&rig, "every 1h", NOW - 3 * HOUR);
    let (_ws, cx) = open(&rig, cx);
    set_settings(
        ScheduledSettings {
            skip_missed: true,
            ..Default::default()
        },
        cx,
    );
    tick(cx);
    cx.run_until_parked();
    assert!(runs(&rig, t.id).is_empty());
    let after = read(&rig.f.store, t.id);
    assert_eq!(after.next_run_at, Some(NOW + HOUR));
    assert_eq!(after.last_status.as_deref(), Some("skipped"));
    assert!(after.enabled, "skipping isn't pausing");
}
