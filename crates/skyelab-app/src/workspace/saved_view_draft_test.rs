//! A new thread's saved view names the thread once its first prompt gives it a row.
#![cfg(unix)]
use super::threads_test::{
    fixture, open_workspace, project_thread, setup, type_and_send, wait_done, wait_until,
};

#[gpui::test]
fn a_new_threads_saved_view_names_the_thread_once_it_starts(cx: &mut gpui::TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let path = f.project_dir.parent().expect("root").join("settings.toml");
    let (ws, cx) = open_workspace(&f, cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.restore_layout(Some(path.clone()), None, window, cx)
    });
    let a = project_thread(&f, &ws, cx);
    type_and_send(&a, cx);
    wait_done(&a, cx);
    let id = a.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(5));
    wait_until(cx, "a saved layout", |_| {
        skyelab_core::AppSettings::load(&path)
            .ok()
            .and_then(|s| s.window)
            .is_some()
    });
    let view = skyelab_core::AppSettings::load(&path)
        .expect("load")
        .window
        .expect("window")
        .view;
    assert_eq!(view, format!("thread:{id}"), "SAVED_VIEW_STALE");
}
