use super::threads_test::test_backend;
use super::*;
use gpui::TestAppContext;

fn setup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
}

#[gpui::test]
fn open_browser_adds_center_tabs_and_close_drops_them(cx: &mut TestAppContext) {
    setup(cx);
    let home = tempfile::tempdir().expect("tempdir");
    let folder = home.path().to_path_buf();
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let ws = Workspace::new(
            folder,
            None,
            AccountsConfig::default(),
            None,
            test_backend(),
            window,
            cx,
        );
        window.focus(&ws.focus, cx);
        ws
    });
    cx.run_until_parked();

    cx.simulate_keystrokes(OPEN_BROWSER_KEY);
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| ws.focus.focus(window, cx));
    cx.simulate_keystrokes(OPEN_BROWSER_KEY);
    cx.run_until_parked();
    let panels = ws.read_with(cx, |ws, _| ws.browsers.clone());
    assert_eq!(panels.len(), 2, "each press adds a browser tab");
    // A tab with nothing asked for says so (T3's "No preview yet"), not a blank pane.
    assert!(cx.debug_bounds("browser-no-page").is_some(), "the empty tab is explained");
    assert!(cx.debug_bounds("browser-open-local").is_some(), "with one click to the dev server");
    let first = panels[0].upgrade().expect("first tab alive");
    ws.read_with(cx, |ws, cx| {
        let dock = ws.dock.read(cx);
        assert!(
            dock.panel(first.entity_id().into()).is_some(),
            "the browser lives in the dock"
        );
    });

    let browser = first.read_with(cx, |p, _| p.browser.downgrade());
    ws.update_in(cx, |ws, window, cx| {
        ws.dock
            .update(cx, |dock, cx| dock.remove_panel(first.clone(), window, cx));
    });
    drop(first);
    cx.run_until_parked();
    assert!(
        browser.upgrade().is_none(),
        "closing the tab drops the browser and its webview"
    );
}

#[gpui::test]
fn agent_calls_refuse_file_urls_and_unloaded_pages(cx: &mut TestAppContext) {
    setup(cx);
    let (browser, cx) =
        cx.add_window_view(|window, cx| skyelab_browser::BrowserView::new(None, window, cx));
    let err = browser
        .update(cx, |b, cx| b.navigate("file:///etc/hosts", cx))
        .expect_err("agent navigate to a file");
    assert!(
        format!("{err:#}").contains("http"),
        "agent refusal, got: {err:#}"
    );

    let eval = browser.read_with(cx, |b, cx| b.eval_js("document.title", cx));
    let err = cx
        .foreground_executor()
        .block_test(eval)
        .expect_err("eval before the first load");
    assert!(
        format!("{err:#}").contains("not loaded"),
        "agent refusal, got: {err:#}"
    );
}

#[test]
fn browser_tab_titles_follow_the_page_and_stay_short() {
    use super::panels::browser_tab_title as t;
    assert_eq!(t(None, false), "Browser");
    assert_eq!(t(Some("  "), false), "Browser", "an empty title keeps the kind");
    assert_eq!(t(Some("QA page"), false), "QA page");
    assert_eq!(t(Some("QA page"), true), "Agent: QA page");
    assert_eq!(t(None, true), "Agent browser");
    let long = t(Some("A very long page title that goes on and on"), false);
    assert_eq!(long, "A very long page title that…");
    assert_eq!(t(Some("ééééééééééééééééééééééééééééééé"), false).chars().count(), 29);
}
