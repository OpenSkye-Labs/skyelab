//! The browser tools on the UI thread. A thread drives only the tab it opened (`BrowserPanel::agent` names it):
//! never the user's tabs, never another thread's, and not one the user took over. Every call goes through
//! skyelab-browser's agent path (http and https only, the in-page guard on eval).

use super::Workspace;
use super::orchestrate::Reply;
use super::panels::{AgentOwner, BrowserPanel};
use futures::FutureExt as _;
use gpui::{App, AppContext as _, AsyncApp, Context, Entity, WeakEntity, Window};
use serde_json::{Value, json};
use skyelab_agent::mcp::BrowserTool;
use std::time::Duration;

/// An eval, or a wait for a page to commit, ends after this.
pub(super) const BROWSER_TIMEOUT: Duration = Duration::from_secs(10);
/// Largest eval result, in bytes of JSON text, handed back to the agent. Errors are cut to this too.
pub(super) const MAX_EVAL_OUTPUT: usize = 64 << 10;
const TICK: Duration = Duration::from_millis(50);

impl Workspace {
    pub(super) fn browser_call(
        &mut self,
        thread_id: i64,
        tool: BrowserTool,
        reply: Reply<Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = self.agent_tab(thread_id, cx);
        let result = match tool {
            BrowserTool::Open { url } => {
                return self.agent_open(thread_id, url, tab, reply, window, cx);
            }
            BrowserTool::Navigate { url } => match tab {
                Ok(tab) => return navigate_and_wait(tab, thread_id, url, false, reply, cx),
                Err(e) => Err(e),
            },
            BrowserTool::Url => tab.and_then(|t| {
                let b = t.read(cx).browser.read(cx);
                b.current_url(cx)
                    .map(|url| json!({"url": url}))
                    .map_err(|e| format!("{e:#}"))
            }),
            BrowserTool::Eval { expression } => match tab {
                Ok(tab) => return eval(tab, thread_id, &expression, reply, cx),
                Err(e) => Err(e),
            },
        };
        let _ = reply.send(result);
    }

    /// The tab `thread_id`'s agent owns. Nothing else counts, so there is no fallback to another tab.
    fn agent_tab(&self, thread_id: i64, cx: &App) -> Result<Entity<BrowserPanel>, String> {
        self.browsers
            .iter()
            .filter_map(|b| b.upgrade())
            .find(|p| owns(p, thread_id, cx))
            .ok_or_else(|| {
                "you have no browser tab (never opened, closed, or the user took it over); call browser_open".into()
            })
    }

    /// Navigates the thread's tab, or opens one, and answers once the page commits.
    fn agent_open(
        &mut self,
        thread_id: i64,
        url: String,
        tab: Result<Entity<BrowserPanel>, String>,
        reply: Reply<Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Ok(tab) = tab {
            return navigate_and_wait(tab, thread_id, url, true, reply, cx);
        }
        // Checked before any webview exists, so a refused URL leaves nothing behind.
        if let Err(e) = skyelab_browser::parse_agent_url(&url) {
            let _ = reply.send(Err(format!("{e:#}")));
            return;
        }
        let browser = self.new_agent_browser(window, cx);
        if let Err(e) = browser.update(cx, |b, cx| b.navigate(&url, cx)) {
            browser.update(cx, |b, cx| b.close(cx));
            let _ = reply.send(Err(format!("{e:#}")));
            return;
        }
        let title = self.open_view(thread_id, cx).map_or_else(
            || format!("thread {thread_id}").into(),
            |v| v.read(cx).title(),
        );
        let tab = self.add_browser(browser, Some(AgentOwner { thread_id, title }), window, cx);
        wait_for_load(&tab, thread_id, url, false, 0, reply, cx);
    }
}

fn owns(tab: &Entity<BrowserPanel>, thread_id: i64, cx: &App) -> bool {
    tab.read(cx)
        .agent
        .as_ref()
        .is_some_and(|a| a.thread_id == thread_id)
}

/// Whether the thread still owns the tab: an answer for a tab the user closed or took over goes nowhere.
fn still_owned(
    tab: &WeakEntity<BrowserPanel>,
    thread_id: i64,
    cx: &AsyncApp,
) -> Result<(), String> {
    match tab.upgrade() {
        Some(tab) if cx.read_entity(&tab, |_, cx| owns(&tab, thread_id, cx)) => Ok(()),
        _ => Err("the user closed or took over your browser tab".into()),
    }
}

/// Starts loading `url` in the thread's tab and answers once the new page commits, so an eval right after
/// runs in it.
fn navigate_and_wait(
    tab: Entity<BrowserPanel>,
    thread_id: i64,
    url: String,
    reused: bool,
    reply: Reply<Value>,
    cx: &mut App,
) {
    let browser = tab.read(cx).browser.clone();
    let before = browser.read(cx).load_count();
    match browser.update(cx, |b, cx| b.navigate(&url, cx)) {
        Ok(()) => wait_for_load(&tab, thread_id, url, reused, before, reply, cx),
        Err(e) => _ = reply.send(Err(format!("{e:#}"))),
    }
}

/// Answers once the tab has committed more than `before` pages, or with `loaded: false` after
/// [`BROWSER_TIMEOUT`].
fn wait_for_load(
    tab: &Entity<BrowserPanel>,
    thread_id: i64,
    url: String,
    reused: bool,
    before: u64,
    reply: Reply<Value>,
    cx: &mut App,
) {
    let browser = tab.read(cx).browser.clone();
    let tab = tab.downgrade();
    cx.spawn(async move |cx| {
        // Counted in ticks, not wall time, so tests on a fake clock see the same deadline.
        for _ in 0..BROWSER_TIMEOUT.as_millis() / TICK.as_millis() {
            if let Err(e) = still_owned(&tab, thread_id, cx) {
                let _ = reply.send(Err(e));
                return;
            }
            if cx.read_entity(&browser, |b, _| b.load_count()) > before {
                let _ = reply.send(Ok(json!({"url": url, "loaded": true, "reused": reused})));
                return;
            }
            cx.background_executor().timer(TICK).await;
        }
        let _ = reply.send(Ok(json!({"url": url, "loaded": false, "reused": reused})));
    })
    .detach();
}

fn eval(
    tab: Entity<BrowserPanel>,
    thread_id: i64,
    expression: &str,
    reply: Reply<Value>,
    cx: &mut App,
) {
    let eval = tab.read(cx).browser.read(cx).eval_js(expression, cx);
    let timeout = cx.background_executor().timer(BROWSER_TIMEOUT);
    let tab = tab.downgrade();
    cx.spawn(async move |cx| {
        let result = futures::select! {
            r = eval.fuse() => r.map_err(|e| cut(format!("{e:#}"))).and_then(refuse_oversized),
            () = timeout.fuse() => Err(format!(
                "the script did not finish in {}s", BROWSER_TIMEOUT.as_secs()
            )),
        };
        let _ = reply.send(still_owned(&tab, thread_id, cx).and(result));
    })
    .detach();
}

/// Refuses a result too big to hand back, rather than cutting JSON in half.
fn refuse_oversized(v: Value) -> Result<Value, String> {
    let n = v.to_string().len();
    if n > MAX_EVAL_OUTPUT {
        return Err(format!(
            "the result is {n} bytes, over the {MAX_EVAL_OUTPUT}-byte limit; return less (a slice or a summary)"
        ));
    }
    Ok(v)
}

/// An error message from the page (a thrown value) can be any size.
fn cut(mut e: String) -> String {
    if e.len() > MAX_EVAL_OUTPUT {
        e.truncate(e.floor_char_boundary(MAX_EVAL_OUTPUT - 3));
        e.push('…');
    }
    e
}
