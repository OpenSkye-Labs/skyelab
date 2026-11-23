//! Browser tabs in the center dock: the user's (⌥⌘B) and agents' (`browser_agent`).

use super::Workspace;
use super::panels::{AgentOwner, BrowserPanel, CloseBrowser};
use gpui::{AppContext as _, Context, Entity, Focusable as _, Window};

use skyelab_browser::BrowserView;

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
        let browser = cx.new(|cx| BrowserView::new(url.as_deref(), window, cx));
        let panel = self.add_browser(browser, None, window, cx);
        panel.read(cx).focus_handle(cx).focus(window, cx);
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
        let panel = cx.new(|_| BrowserPanel { browser, agent });
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
