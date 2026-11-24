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
    cx.simulate_keystrokes(skyelab_browser::ZOOM_IN_KEY);
    assert_eq!(zoom(cx), 1.1);
    cx.simulate_keystrokes(skyelab_browser::ZOOM_IN_KEY);
    assert_eq!(zoom(cx), 1.25);
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
fn duplicate_opens_user_tab(cx: &mut TestAppContext) {
    let (ws, _browser, _page, cx) = agent_tab("http://localhost:3000/a", cx);
    cx.dispatch_action(skyelab_browser::BrowserDuplicate);
    cx.run_until_parked();
    let all = tabs(&ws, cx);
    assert_eq!(all.len(), 2, "a second tab");
    let copy = all[1].read_with(cx, |p, _| (p.agent.is_none(), p.browser.clone()));
    assert!(copy.0, "the copy is the user's, never the agent's");
    assert_eq!(
        copy.1.read_with(cx, |b, cx| b.url_bar_text(cx)),
        "http://localhost:3000/a"
    );
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
