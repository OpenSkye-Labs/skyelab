//! Acceptance 10: the sidebar's "Scheduled" badge comes from the stored row, so it shows after a restart with no view
//! of the thread ever opened; a thread the user started has none.

use super::super::lock;
use super::test_util::{NOW, open, rig, stored_run, task};
use gpui::TestAppContext;

#[gpui::test]
fn stored_scheduled_threads_get_the_badge(cx: &mut TestAppContext) {
    let rig = rig(true);
    let t = task(&rig, "every 1h", NOW + 1);
    let scheduled = stored_run(&rig, t.id, "idle");
    let plain = lock(&rig.f.store)
        .expect("store")
        .create_thread(&skyelab_store::NewThread {
            project_id: rig.project,
            title: "by hand",
            cwd: &rig.f.project_dir,
            account_id: "acme",
            status: "idle",
        })
        .expect("thread")
        .id;
    let (ws, cx) = open(&rig, cx);
    ws.update(cx, |ws, cx| ws.refresh_sidebar(cx));
    let badge = |id: i64, cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| {
            let key = format!("thread-{id}");
            ws.sidebar
                .read(cx)
                .marks
                .get(key.as_str())
                .map(|m| m.scheduled)
        })
    };
    assert_eq!(badge(scheduled, cx), Some(true));
    assert_eq!(badge(plain, cx), Some(false));
}
