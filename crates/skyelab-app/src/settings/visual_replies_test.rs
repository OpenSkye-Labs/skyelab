//! Acceptance: Settings → Chat → Visual replies adds only hosts the validator takes, saves them to settings.toml,
//! and Reset brings back the CDNs.

use super::*;
use crate::workspace::thread_extras::prefs;
use gpui::TestAppContext;
use skyelab_core::{AppSettings, VisualReplySettings};

#[gpui::test]
fn hosts_are_validated_saved_and_reset(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
    });
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("settings.toml");
    cx.update(|cx| prefs::install(Some(file.clone()), cx));
    let path = tmp.path().join("accounts.toml");
    let (view, cx) = cx.add_window_view(|window, cx| {
        Settings::new(Some(path), AccountsConfig::default(), None, window, cx)
    });
    let typed = |text: &'static str, view: &Entity<Settings>, cx: &mut gpui::VisualTestContext| {
        view.update_in(cx, |v, window, cx| {
            v.page_host_input
                .update(cx, |i, cx| i.set_value(text, window, cx));
            v.add_page_host(window, cx);
        });
        cx.run_until_parked();
    };
    let defaults = VisualReplySettings::default().allowed_hosts;
    for bad in [
        "localhost",
        "*",
        "http://cdn.example.com",
        "10.0.0.1",
        "cdn.example.com/x",
    ] {
        typed(bad, &view, cx);
        assert_eq!(cx.update(|_, cx| prefs::page_hosts(cx)), defaults, "{bad}");
        let error = view.read_with(cx, |v, _| v.threads_error.clone());
        assert!(
            error.is_some_and(|e| e.contains("can't be allowed")),
            "{bad}"
        );
    }
    typed("https://Fonts.GStatic.com/", &view, cx);
    let mut want = defaults.clone();
    want.push("fonts.gstatic.com".into());
    assert_eq!(cx.update(|_, cx| prefs::page_hosts(cx)), want);
    let saved = AppSettings::load(&file).expect("load");
    assert_eq!(saved.visual_replies.allowed_hosts, want);
    view.update_in(cx, |v, _, cx| {
        v.set_page_hosts(VisualReplySettings::default().allowed_hosts, cx)
    });
    cx.run_until_parked();
    let saved = AppSettings::load(&file).expect("load");
    assert_eq!(saved.visual_replies.allowed_hosts, defaults);
}
