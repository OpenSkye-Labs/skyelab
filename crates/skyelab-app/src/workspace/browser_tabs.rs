//! Browser tabs in the center dock: the user's (⌥⌘B) and agents' (`browser_agent`).

use super::Workspace;
use super::panels::{AgentOwner, BrowserPanel, CloseBrowser};
use gpui::{AppContext as _, Context, Entity, Focusable as _, Window};

use skyelab_browser::{BrowserView, DuplicateTab};

/// Makes a fake page engine and its load reports, for a test's agent tabs.
#[cfg(test)]
pub type FakeEngineFactory = std::rc::Rc<
    dyn Fn() -> (
        std::rc::Rc<dyn skyelab_browser::Engine>,
        flume::Receiver<String>,
    ),
>;

impl Workspace {
    pub(super) fn open_browser(
        &mut self,
        _: &super::OpenBrowser,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let url = std::env::var("SKYELAB_BROWSER_URL").ok();
        self.open_browser_at(url.as_deref(), window, cx);
    }

    /// A user browser tab at `url` (a blank one without), focused.
    pub(super) fn open_browser_at(
        &mut self,
        url: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let browser = self.user_browser(url, window, cx);
        let panel = self.add_browser(browser, None, window, cx);
        panel.read(cx).focus_handle(cx).focus(window, cx);
    }

    /// A user's tab at `url`, private when it is a forwarded port's page (`RemoteEnv::forwards_page`).
    fn user_browser(
        &self,
        url: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<BrowserView> {
        let forwarded = url.is_some_and(|url| {
            self.remote_env()
                .is_some_and(|env| env.read(cx).forwards_page(url))
        });
        match forwarded {
            true => cx.new(|cx| BrowserView::new_private(url, window, cx)),
            false => cx.new(|cx| BrowserView::new(url, window, cx)),
        }
    }

    /// A blank browser pane for an agent tab: a test's fake page, or a real webview.
    pub(super) fn new_agent_browser(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<BrowserView> {
        #[cfg(test)]
        if let Some(make) = &self.browser_engine {
            let (engine, loads) = make();
            return cx.new(|cx| BrowserView::with_engine(engine, loads, window, cx));
        }
        cx.new(|cx| BrowserView::new_agent(window, cx))
    }

    /// Adds `browser` as a tab in the right dock, owned by `agent`'s thread or (`None`) the user's.
    pub(super) fn add_browser(
        &mut self,
        browser: Entity<BrowserView>,
        agent: Option<AgentOwner>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<BrowserPanel> {
        if agent.is_some() {
            browser.update(cx, |b, _| b.mark_agent_page());
        }
        let private = browser.read(cx).is_private();
        // A copy is never the agent's. A copy of a private tab (an agent's page, taken over or not, or a forwarded
        // page, whether or not the forward still exists) is private too, so none ever lands in the user's profile.
        cx.subscribe_in(
            &browser,
            window,
            |this, source, DuplicateTab(url): &DuplicateTab, window, cx| {
                let (private, agent_page) = {
                    let source = source.read(cx);
                    (source.is_private(), source.is_agent_page())
                };
                let copy = match private {
                    true => cx.new(|cx| {
                        let mut copy = BrowserView::new_private(url.as_deref(), window, cx);
                        if agent_page {
                            copy.mark_agent_page();
                        }
                        copy
                    }),
                    false => this.user_browser(url.as_deref(), window, cx),
                };
                let panel = this.add_browser(copy, None, window, cx);
                panel.read(cx).focus_handle(cx).focus(window, cx);
            },
        )
        .detach();
        let panel = cx.new(|_| BrowserPanel {
            users: !private,
            browser,
            agent,
        });
        self.add_right_panel(panel.clone(), window, cx);
        cx.subscribe_in(
            &panel,
            window,
            |this, panel, _: &CloseBrowser, window, cx| {
                this.dock
                    .update(cx, |dock, cx| dock.remove_panel(panel.clone(), window, cx));
            },
        )
        .detach();
        self.browsers.retain(|b| b.upgrade().is_some());
        self.browsers.push(panel.downgrade());
        panel
    }
}
