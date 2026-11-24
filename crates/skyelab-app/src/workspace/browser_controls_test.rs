//! The browser tab's own controls (zoom, find in page, the ⋯ menu's commands) on a fake page engine.

use super::browser_agent_test::{call, open_url, tabs, workspace};
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_browser::{BrowserView, Engine, EvalDone, FindDone};
use std::cell::RefCell;
use std::rc::Rc;

/// A page that only loads and answers find: "hello" is on it, nothing else is.
#[derive(Default)]
struct Page {
    url: RefCell<String>,
    loads: RefCell<Option<flume::Sender<String>>>,
    finds: RefCell<Vec<(String, bool, bool)>>,
    /// Set: find answers wait here instead of landing at once.
    held: RefCell<Option<Option<FindDone>>>,
}

impl Engine for Page {
    fn load(&self, url: &str, _: &gpui::App) -> anyhow::Result<()> {
        *self.url.borrow_mut() = url.to_owned();
        if let Some(loads) = &*self.loads.borrow() {
            loads.send(url.to_owned())?;
        }
        Ok(())
    }
    fn url(&self, _: &gpui::App) -> anyhow::Result<String> {
        Ok(self.url.borrow().clone())
    }
    fn eval(&self, _: &str, _: EvalDone, _: &gpui::App) -> anyhow::Result<()> {
        anyhow::bail!("not used here")
    }
    fn find(
        &self,
        query: &str,
        backwards: bool,
        restart: bool,
        done: FindDone,
        _: &gpui::App,
    ) -> anyhow::Result<()> {
        (self.finds.borrow_mut()).push((query.to_owned(), backwards, restart));
        if let Some(held) = &mut *self.held.borrow_mut() {
            *held = Some(done);
            return Ok(());
        }
        done(query == "hello");
        Ok(())
    }
}

/// An agent tab on a fake page at `url`, focused, and its page.
fn agent_tab<'a>(
    url: &str,
    cx: &'a mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<BrowserView>,
    Rc<Page>,
    &'a mut VisualTestContext,
) {
    let (ws, cx) = workspace(cx);
    let pages: Rc<RefCell<Vec<Rc<Page>>>> = Rc::default();
    let made = pages.clone();
    ws.update(cx, |ws, _| {
        ws.browser_engine = Some(Rc::new(move || {
            let (tx, rx) = flume::unbounded();
            let page = Rc::new(Page::default());
            *page.loads.borrow_mut() = Some(tx);
            made.borrow_mut().push(page.clone());
            (page as Rc<dyn Engine>, rx)
        }))
    });
    call(&ws, 1, open_url(url), cx).expect("open");
    let browser = tabs(&ws, cx)[0].read_with(cx, |p, _| p.browser.clone());
    browser.update_in(cx, |b, window, cx| b.focus_handle(cx).focus(window, cx));
    cx.run_until_parked();
    let page = pages.borrow()[0].clone();
    (ws, browser, page, cx)
}

#[gpui::test]
fn zoom_keys(cx: &mut TestAppContext) {
    let (_ws, browser, _page, cx) = agent_tab("http://localhost:3000/", cx);
    let zoom = |cx: &mut VisualTestContext| browser.read_with(cx, |b, _| b.zoom());
    assert_eq!(zoom(cx), 1.0);
    assert_eq!(
        browser.read_with(cx, |b, _| b.zoom_label()),
        None,
        "no chip at 100%"
    );
    cx.simulate_keystrokes(skyelab_browser::ZOOM_IN_KEY);
    assert_eq!(zoom(cx), 1.1);
    cx.simulate_keystrokes(skyelab_browser::ZOOM_IN_KEY);
    assert_eq!(zoom(cx), 1.25);
    assert_eq!(
        browser.read_with(cx, |b, _| b.zoom_label()).as_deref(),
        Some("125%")
    );
    cx.simulate_keystrokes(skyelab_browser::ZOOM_RESET_KEY);
    assert_eq!(zoom(cx), 1.0);
    cx.simulate_keystrokes(skyelab_browser::ZOOM_OUT_KEY);
    assert_eq!(zoom(cx), 0.9);
}

#[gpui::test]
fn find_in_page(cx: &mut TestAppContext) {
    let (_ws, browser, page, cx) = agent_tab("http://localhost:3000/", cx);
    assert_eq!(browser.read_with(cx, |b, cx| b.find_query(cx)), None);
    cx.simulate_keystrokes(skyelab_browser::FIND_KEY);
    cx.run_until_parked();
    assert_eq!(
        browser.read_with(cx, |b, cx| b.find_query(cx)),
        Some(String::new()),
        "the bar opens"
    );

    cx.simulate_input("nope");
    cx.run_until_parked();
    assert_eq!(
        page.finds.borrow().last().cloned(),
        Some(("nope".into(), false, true)),
        "typing searches from the top of the current match"
    );
    assert!(
        browser.read_with(cx, |b, _| b.find_missed()),
        "a miss shows"
    );

    browser.update_in(cx, |b, window, cx| b.set_find_query("hello", window, cx));
    cx.run_until_parked();
    assert!(!browser.read_with(cx, |b, _| b.find_missed()));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        page.finds.borrow().last().cloned(),
        Some(("hello".into(), false, false)),
        "Enter steps to the next match: {:?}",
        page.finds.borrow()
    );

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(
        browser.read_with(cx, |b, cx| b.find_query(cx)),
        None,
        "Esc closes"
    );
    assert_eq!(
        page.finds.borrow().last().map(|f| f.0.clone()),
        Some(String::new()),
        "closing clears the page's match"
    );
}

#[gpui::test]
fn duplicate_of_an_agent_tab_opens_a_private_user_tab(cx: &mut TestAppContext) {
    let (ws, _browser, _page, cx) = agent_tab("http://localhost:3000/a", cx);
    cx.dispatch_action(skyelab_browser::BrowserDuplicate);
    cx.run_until_parked();
    let all = tabs(&ws, cx);
    assert_eq!(all.len(), 2, "a second tab");
    let (agent, users, copy) = all[1].read_with(cx, |p, _| (p.agent.is_none(), p.users, p.browser.clone()));
    assert!(agent, "the copy is the user's, never the agent's");
    assert!(!users, "closing it never reopens it in the user's profile");
    assert!(
        copy.read_with(cx, |b, _| b.is_agent_page()),
        "the copy stays private, and so do its own copies"
    );
    assert_eq!(
        copy.read_with(cx, |b, cx| b.url_bar_text(cx)),
        "http://localhost:3000/a"
    );
    // A copy of the copy is private too.
    copy.update_in(cx, |b, window, cx| b.focus_handle(cx).focus(window, cx));
    cx.dispatch_action(skyelab_browser::BrowserDuplicate);
    cx.run_until_parked();
    let all = tabs(&ws, cx);
    assert_eq!(all.len(), 3);
    assert!(all[2].read_with(cx, |p, cx| !p.users && p.browser.read(cx).is_agent_page()));
}

#[gpui::test]
fn duplicate_of_a_user_tab_stays_in_the_users_profile(cx: &mut TestAppContext) {
    let (ws, cx) = workspace(cx);
    cx.dispatch_action(OpenBrowser);
    cx.run_until_parked();
    cx.dispatch_action(skyelab_browser::BrowserDuplicate);
    cx.run_until_parked();
    let all = tabs(&ws, cx);
    assert_eq!(all.len(), 2);
    assert!(all[1].read_with(cx, |p, cx| p.users && !p.browser.read(cx).is_agent_page()));
}

#[gpui::test]
fn an_agent_page_never_opens_in_the_system_browser(cx: &mut TestAppContext) {
    let (ws, _browser, _page, cx) = agent_tab("https://example.com/a", cx);
    cx.dispatch_action(skyelab_browser::BrowserOpenExternal);
    cx.run_until_parked();
    assert_eq!(cx.opened_url(), None, "the agent's tab");
    // Taken over, it is still the agent's page.
    tabs(&ws, cx)[0].update(cx, |p, cx| p.take_over(cx));
    cx.dispatch_action(skyelab_browser::BrowserOpenExternal);
    cx.run_until_parked();
    assert_eq!(cx.opened_url(), None, "after Take over");
}

#[gpui::test]
fn a_private_page_never_opens_in_the_system_browser(cx: &mut TestAppContext) {
    let (_ws, browser, _page, cx) = agent_tab("https://example.com/a", cx);
    browser.update(cx, |b, _| assert!(b.is_private(), "an agent's page is private"));
    cx.dispatch_action(skyelab_browser::BrowserOpenExternal);
    cx.run_until_parked();
    assert_eq!(cx.opened_url(), None);
}

#[gpui::test]
fn copy_url(cx: &mut TestAppContext) {
    let (_ws, _browser, _page, cx) = agent_tab("http://localhost:3000/b", cx);
    cx.dispatch_action(skyelab_browser::BrowserCopyUrl);
    let copied = cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(copied.as_deref(), Some("http://localhost:3000/b"));
}

#[gpui::test]
fn copy_url_needs_a_page(cx: &mut TestAppContext) {
    let (ws, cx) = workspace(cx);
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("before".into()));
    cx.dispatch_action(OpenBrowser);
    cx.run_until_parked();
    assert_eq!(tabs(&ws, cx).len(), 1, "a blank user tab, focused");
    cx.dispatch_action(skyelab_browser::BrowserCopyUrl);
    let copied = cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(
        copied.as_deref(),
        Some("before"),
        "nothing loaded, nothing copied"
    );
}

#[gpui::test]
fn a_find_answer_from_the_old_page_does_not_land_on_the_new_one(cx: &mut TestAppContext) {
    let (_ws, browser, page, cx) = agent_tab("http://localhost:3000/", cx);
    cx.simulate_keystrokes(skyelab_browser::FIND_KEY);
    cx.run_until_parked();
    *page.held.borrow_mut() = Some(None);
    cx.simulate_input("nope");
    cx.run_until_parked();
    // The page navigates before the old page's miss arrives.
    browser
        .update(cx, |b, cx| b.navigate("http://localhost:3000/next", cx))
        .expect("load");
    cx.run_until_parked();
    let done = page
        .held
        .borrow_mut()
        .take()
        .flatten()
        .expect("a find waits");
    done(false);
    cx.run_until_parked();
    assert!(
        !browser.read_with(cx, |b, _| b.find_missed()),
        "STALE_FIND: the old page's miss shows on the new page"
    );
}

/// A private tab that isn't an agent's (a forwarded port's page): private is the tab's own, not the forward's.
#[gpui::test]
fn a_private_tab_stays_private_after_its_forward_is_gone(cx: &mut TestAppContext) {
    let (ws, cx) = workspace(cx);
    ws.update_in(cx, |ws, window, cx| {
        let tab = cx.new(|cx| BrowserView::new_private(Some("http://127.0.0.1:4100/"), window, cx));
        let panel = ws.add_browser(tab, None, window, cx);
        panel.read(cx).focus_handle(cx).focus(window, cx);
    });
    cx.run_until_parked();
    let first = tabs(&ws, cx)[0].clone();
    assert!(
        first.read_with(cx, |p, cx| !p.users && p.browser.read(cx).is_private()),
        "closing it never reopens it in the user's profile"
    );
    // No forward exists in this window: the copy is private because its source is.
    cx.dispatch_action(skyelab_browser::BrowserDuplicate);
    cx.run_until_parked();
    let all = tabs(&ws, cx);
    assert_eq!(all.len(), 2);
    assert!(all[1].read_with(cx, |p, cx| !p.users
        && p.browser.read(cx).is_private()
        && !p.browser.read(cx).is_agent_page()));
}

#[gpui::test]
fn a_failed_load_shows_why_and_retry_loads_it_again(cx: &mut TestAppContext) {
    let (_ws, browser, page, cx) = agent_tab("http://localhost:3000/", cx);
    let failed = skyelab_browser::load_error::describe(
        "NSURLErrorDomain",
        -1004,
        "Could not connect to the server.",
        "http://localhost:3000/",
    );
    browser.update(cx, |b, cx| b.load_failed(failed, cx));
    cx.run_until_parked();
    assert!(cx.debug_bounds("browser-load-error").is_some(), "the error page is drawn");
    let shown = browser.read_with(cx, |b, _| b.load_error().cloned()).expect("an error");
    assert_eq!(shown.host, "localhost:3000");
    assert!(shown.hint.is_some_and(|h| h.contains("dev server")));

    // A cancelled load (the next navigation took over) is not an error and leaves this one up.
    let cancelled = skyelab_browser::load_error::describe("NSURLErrorDomain", -999, "", "http://localhost:3000/");
    browser.update(cx, |b, cx| b.load_failed(cancelled, cx));
    assert!(browser.read_with(cx, |b, _| b.load_error().is_some()));

    *page.url.borrow_mut() = "about:blank".into();
    let at = cx.debug_bounds("browser-load-retry").expect("Retry is drawn");
    cx.simulate_click(at.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(*page.url.borrow(), "http://localhost:3000/", "Retry loads the failed address");
    // The fake commits each load at once: the page is back.
    assert!(browser.read_with(cx, |b, _| b.load_error().is_none()), "a committed page clears it");
    assert!(cx.debug_bounds("browser-load-error").is_none());
}

#[gpui::test]
fn back_from_the_error_page_shows_the_page_underneath(cx: &mut TestAppContext) {
    let (_ws, browser, _page, cx) = agent_tab("http://localhost:3000/", cx);
    let failed = skyelab_browser::load_error::describe("NSURLErrorDomain", -1003, "", "https://nope.invalid/");
    browser.update(cx, |b, cx| b.load_failed(failed, cx));
    cx.run_until_parked();
    let at = cx.debug_bounds("browser-back").expect("← is drawn");
    cx.simulate_click(at.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(browser.read_with(cx, |b, _| b.load_error().is_none()), "← dismisses the error");
}

#[gpui::test]
fn an_agent_navigate_that_fails_answers_with_why(cx: &mut TestAppContext) {
    let (ws, browser, page, cx) = agent_tab("http://localhost:3000/", cx);
    *page.loads.borrow_mut() = None; // nothing commits from here on
    let (reply, answer) = flume::bounded(1);
    ws.update_in(cx, |ws, window, cx| {
        let tool = skyelab_agent::mcp::BrowserTool::Navigate {
            url: "http://localhost:4000/".into(),
        };
        ws.browser_call(1, tool, reply, window, cx)
    });
    cx.run_until_parked();
    assert!(answer.try_recv().is_err(), "still waiting for the page");
    let failed = skyelab_browser::load_error::describe(
        "NSURLErrorDomain",
        -1004,
        "Could not connect to the server.",
        "http://localhost:4000/",
    );
    browser.update(cx, |b, cx| b.load_failed(failed, cx));
    cx.executor().advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    let result = answer.try_recv().expect("answered at the failure, not the timeout").expect("ok");
    assert_eq!(result["loaded"], false);
    let error = result["error"].as_str().expect("a reason");
    assert!(error.contains("localhost:4000") && error.contains("refused"), "{error}");
}
