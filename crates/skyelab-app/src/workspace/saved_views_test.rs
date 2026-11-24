//! The sidebar's saved views: save the scope and filter, change both, pick the view back, delete it.

use super::super::manage_harness::rig;
use super::super::threads_test::wait_until;
use super::{ApplySidebarView, DeleteSidebarView, SaveSidebarView};
use gpui::TestAppContext;
use skyelab_core::AppSettings;

#[gpui::test]
fn a_saved_view_restores_scope_and_filter_and_deletes_from_the_file(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let dir = tempfile::tempdir().expect("tempdir");
    let settings = dir.path().join("settings.toml");
    std::fs::write(&settings, "editor = \"zed\"\n").expect("seed");
    cx.update(|_, cx| super::super::thread_extras::prefs::install(Some(settings.clone()), cx));
    r.thread("alpha", cx);
    r.thread("beta", cx);
    let project = r.project;
    let path = r.f.project_dir.clone();
    r.ws.update_in(cx, |ws, window, cx| {
        ws.scope = vec![project];
        let search = ws.sidebar.read(cx).search.clone();
        search.update(cx, |s, cx| s.set_value("alp", window, cx));
        ws.search_threads("alp".into(), cx);
    });
    wait_until(cx, "filtered", |cx| r.listed(cx) == ["alpha"]);

    cx.dispatch_action(SaveSidebarView);
    assert!(r.dialog_open(cx), "the save asks a name");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    });
    cx.simulate_input("Mine");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let on_disk = || AppSettings::load(&settings).expect("load");
    wait_until(cx, "saved to the file", |_| {
        on_disk().sidebar_views.len() == 1
    });
    let saved = &on_disk().sidebar_views[0];
    assert_eq!(saved.name, "Mine");
    assert_eq!(saved.projects, [path]);
    assert_eq!(saved.search, "alp");
    assert_eq!(
        on_disk().editor.as_deref(),
        Some("zed"),
        "the rest of the file stays"
    );

    // Change both, then pick the view: scope and filter come back, in the input too.
    r.ws.update_in(cx, |ws, window, cx| {
        ws.scope.clear();
        let search = ws.sidebar.read(cx).search.clone();
        search.update(cx, |s, cx| s.set_value("", window, cx));
        ws.search_threads("".into(), cx);
    });
    wait_until(cx, "unfiltered", |cx| r.listed(cx).len() == 2);
    // The menu dispatches with the sidebar's focus as context; the dialog took focus away here.
    r.ws.update_in(cx, |ws, window, cx| {
        let a = ApplySidebarView {
            name: "Mine".into(),
        };
        ws.apply_sidebar_view(&a, window, cx)
    });
    wait_until(cx, "the view applied", |cx| r.listed(cx) == ["alpha"]);
    r.ws.read_with(cx, |ws, cx| {
        assert_eq!(ws.scope, [project]);
        assert_eq!(ws.sidebar.read(cx).search.read(cx).value(), "alp");
        let views = super::views(cx);
        assert!(ws.shows_view(&views[0]));
    });

    r.ws.update_in(cx, |ws, window, cx| {
        let a = DeleteSidebarView {
            name: "Mine".into(),
        };
        ws.delete_sidebar_view(&a, window, cx)
    });
    wait_until(cx, "deleted from the file", |_| {
        on_disk().sidebar_views.is_empty()
    });
    assert!(cx.update(|_, cx| super::views(cx).is_empty()));

    // A file that doesn't parse is left alone, and the save says so.
    std::fs::write(&settings, "not = [valid").expect("break the file");
    r.ws.update_in(cx, |ws, window, cx| {
        ws.save_sidebar_view(&SaveSidebarView, window, cx)
    });
    assert!(r.dialog_open(cx));
    cx.simulate_input("Again");
    cx.simulate_keystrokes("enter");
    wait_until(cx, "the failed save is told", |cx| {
        r.notices(cx)
            .iter()
            .any(|n| n.starts_with("Couldn't save the sidebar views"))
    });
    assert_eq!(
        std::fs::read_to_string(&settings).expect("read"),
        "not = [valid"
    );
}
