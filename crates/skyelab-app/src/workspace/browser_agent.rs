//! The browser tools on the UI thread. A thread drives only the tab it opened (`BrowserPanel::agent` names it):
//! never the user's tabs, never another thread's, and not one the user took over. Every call goes through
//! skyelab-browser's agent path (http and https only, the in-page guard on eval).

use super::Workspace;
use super::orchestrate::Reply;
use super::panels::{AgentOwner, BrowserPanel};
use futures::FutureExt as _;
use gpui::{Context, Entity, Window};
use serde_json::{Value, json};
use skyelab_agent::mcp::BrowserTool;
use std::time::{Duration, Instant};

/// An eval, or waiting for a new tab's first page, gives up after this.
pub(super) const BROWSER_TIMEOUT: Duration = Duration::from_secs(10);
/// Largest eval result (as JSON text) handed back to the agent.
pub(super) const MAX_EVAL_OUTPUT: usize = 64 << 10;

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
        match tool {
            BrowserTool::Open { url } => self.agent_open(thread_id, &url, tab, reply, window, cx),
            BrowserTool::Navigate { url } => {
                _ = reply.send(
                    tab.and_then(|t| navigate(&t, &url, cx))
                        .map(|()| json!({"url": url})),
                )
            }
            BrowserTool::Url => {
                let url = tab.and_then(|t| {
                    let b = t.read(cx).browser.read(cx);
                    b.current_url(cx).map_err(|e| format!("{e:#}"))
                });
                _ = reply.send(url.map(|url| json!({"url": url})));
            }
            BrowserTool::Eval { expression } => {
                let tab = match tab {
                    Ok(tab) => tab,
                    Err(e) => {
                        let _ = reply.send(Err(e));
                        return;
                    }
                };
                let eval = tab.read(cx).browser.read(cx).eval_js(&expression, cx);
                let timeout = cx.background_executor().timer(BROWSER_TIMEOUT);
                cx.spawn(async move |_, _| {
                    let result = futures::select! {
                        r = eval.fuse() => r.map_err(|e| format!("{e:#}")).and_then(capped),
                        () = timeout.fuse() => Err(format!(
                            "the script did not finish in {}s", BROWSER_TIMEOUT.as_secs()
                        )),
                    };
                    _ = reply.send(result);
                })
                .detach();
            }
        }
    }

    /// The tab `thread_id`'s agent owns. Nothing else counts, so there is no fallback to another tab.
    fn agent_tab(&self, thread_id: i64, cx: &gpui::App) -> Result<Entity<BrowserPanel>, String> {
        self.browsers
            .iter()
            .filter_map(|b| b.upgrade())
            .find(|p| p.read(cx).agent.as_ref().is_some_and(|a| a.thread_id == thread_id))
            .ok_or_else(|| {
                "you have no browser tab (never opened, closed, or the user took it over); call browser_open".into()
            })
    }

    /// Navigates the thread's tab, or opens one. A new tab answers once its first page commits, so an eval
    /// right after works.
    fn agent_open(
        &mut self,
        thread_id: i64,
        url: &str,
        tab: Result<Entity<BrowserPanel>, String>,
        reply: Reply<Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Ok(tab) = tab {
            let navigated = navigate(&tab, url, cx).map(|()| json!({"url": url, "reused": true}));
            let _ = reply.send(navigated);
            return;
        }
        // Checked before a tab exists, so a refused URL leaves nothing behind.
        if let Err(e) = skyelab_browser::parse_agent_url(url) {
            let _ = reply.send(Err(format!("{e:#}")));
            return;
        }
        let browser = self.new_agent_browser(window, cx);
        if let Err(e) = browser.update(cx, |b, cx| b.navigate(url, cx)) {
            browser.update(cx, |b, cx| b.close(cx));
            let _ = reply.send(Err(format!("{e:#}")));
            return;
        }
        let title = self.open_view(thread_id, cx).map_or_else(
            || format!("thread {thread_id}").into(),
            |v| v.read(cx).title(),
        );
        self.add_browser(
            browser.clone(),
            Some(AgentOwner { thread_id, title }),
            window,
            cx,
        );
        let url = url.to_owned();
        cx.spawn(async move |_, cx| {
            let deadline = Instant::now() + BROWSER_TIMEOUT;
            loop {
                let loaded = browser.read_with(cx, |b, _| b.is_loaded());
                if loaded || Instant::now() >= deadline {
                    _ = reply.send(Ok(json!({"url": url, "loaded": loaded, "reused": false})));
                    return;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
            }
        })
        .detach();
    }
}

fn navigate(tab: &Entity<BrowserPanel>, url: &str, cx: &mut gpui::App) -> Result<(), String> {
    let browser = tab.read(cx).browser.clone();
    browser
        .update(cx, |b, cx| b.navigate(url, cx))
        .map_err(|e| format!("{e:#}"))
}

/// Refuses a result too big to hand back, rather than cutting JSON in half.
fn capped(v: Value) -> Result<Value, String> {
    let n = v.to_string().len();
    if n > MAX_EVAL_OUTPUT {
        return Err(format!(
            "the result is {n} bytes, over the {MAX_EVAL_OUTPUT}-byte limit; return less (a slice or a summary)"
        ));
    }
    Ok(v)
}
