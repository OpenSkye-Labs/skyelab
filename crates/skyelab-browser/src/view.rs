use std::{future::Future, rc::Rc};

use anyhow::Context as _;
use futures::channel::oneshot;
use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Task, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
};
use gpui_webview::WebView;

use crate::{
    Engine,
    agent::{check_agent_eval, parse_agent_url},
    controls, js, parse_url,
};

/// A browser pane. Agents drive it with `navigate`, `current_url` and `eval_js`, which only reach
/// http and https pages. The URL bar is the user's path and also opens local files.
pub struct BrowserView {
    /// Debug mode's live count of browser tabs.
    _live: skyelab_core::debug::Live,
    /// `None` when the native webview couldn't be built (e.g. a Wayland window); `error` says why.
    webview: Option<Entity<WebView>>,
    /// What loads and runs scripts: the webview, or a test's fake. `None` once closed.
    engine: Option<Rc<dyn Engine>>,
    url_bar: Entity<InputState>,
    error: Option<SharedString>,
    /// A page has committed (wry's `Started`). Until then wry queues scripts for the next page.
    loaded: bool,
    /// A page was asked for: until then the pane shows "No page yet" in place of the empty page.
    asked: bool,
    /// Checks a local address for a listener before loading it (the real page; tests turn it on).
    probe_local: bool,
    /// Pages committed so far, so a caller can wait for the next one.
    loads: u64,
    /// The URL of the page that committed last (wry's `Started`), which the user sees. The engine's own URL can
    /// name a pending load.
    committed_url: Option<String>,
    /// The page's `document.title` after its last load; untrusted page text.
    title: Option<SharedString>,
    /// The last load failed: this shows instead of the (hidden) page until the next page commits.
    load_error: Option<crate::load_error::LoadError>,
    /// Retry was pressed and nothing has answered yet.
    retrying: bool,
    load_failures: u64,
    /// Keeps WebKit's failure reports coming (wry has none).
    #[cfg(target_os = "macos")]
    _failures: Option<skyelab_webkit_sys::FailureWatch>,
    _failed: Task<()>,
    /// The local-port check of the load in flight (see [`Self::load`]).
    _probe: Task<()>,
    focus: FocusHandle,
    zoom: f64,
    /// The host shows the pane ([`Self::set_visible`]); the page is also hidden while the ⋯ menu is open.
    shown: bool,
    menu_open: bool,
    /// On a private, in-memory web store of its own ([`Self::new_private`], an agent's tab). It stays private for
    /// good: its copies are private and it never opens in the system browser or comes back on the user's profile.
    private: bool,
    /// An agent's page, or a copy of one ([`Self::mark_agent_page`]). Always private too.
    agent_page: bool,
    find: Option<controls_ui::Find>,
    _find_task: Task<()>,
    _data_dir: Option<tempfile::TempDir>,
    _subscription: Subscription,
    _sync_url_bar: Task<()>,
    _popups: Task<()>,
    _sync_title: Task<()>,
}

impl BrowserView {
    /// Opens `url` (the user's, validated with [`parse_url`]), or a blank page for `None`, in a new
    /// native webview attached to `window`. A rejected URL or a webview that can't be built shows
    /// as an error in the pane.
    pub fn new(url: Option<&str>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_wry(url, false, window, cx)
    }

    /// [`Self::new`] on a private, in-memory web data store of its own, as an agent's tab gets: for a page of
    /// another machine (a forwarded port), which must not share cookies or storage with this machine's pages.
    pub fn new_private(url: Option<&str>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_wry(url, true, window, cx)
    }

    /// A blank pane for an agent, on a private, in-memory web data store of its own: no cookies,
    /// storage or cache shared with the user's tabs, other agents' tabs or later runs.
    pub fn new_agent(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::with_wry(None, true, window, cx);
        view.agent_page = true;
        // An agent navigates it at once: keep the page laid out from the first frame (a screenshot needs its size).
        view.asked = true;
        view
    }

    /// Marks this as an agent's page, or a copy of one (see `agent_page`). It stays marked after Take over.
    pub fn mark_agent_page(&mut self) {
        self.agent_page = true;
    }

    /// Whether a local address is checked for a listener before it loads (see `load_error::refused_local`): on for
    /// a real page, off for a test's fake engine unless the test turns it on.
    pub fn set_probe_local(&mut self, on: bool) {
        self.probe_local = on;
    }

    pub fn is_agent_page(&self) -> bool {
        self.agent_page
    }

    /// Whether this tab is private (see `private`): built private, or an agent's page.
    pub fn is_private(&self) -> bool {
        self.private || self.agent_page
    }

    /// A pane on `engine` (a test's fake page). `loads` reports each page that commits, as wry's
    /// `Started` does. Nothing is drawn for the page.
    pub fn with_engine(
        engine: Rc<dyn Engine>,
        loads: flume::Receiver<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::build(None, Some(engine), loads, None, None, window, cx)
    }

    fn build(
        webview: Option<Entity<WebView>>,
        engine: Option<Rc<dyn Engine>>,
        loaded_rx: flume::Receiver<String>,
        url: Option<&str>,
        error: Option<anyhow::Error>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let url_bar = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("localhost:3000")
                .default_value(url.unwrap_or_default().trim().to_string())
        });
        let subscription = cx.subscribe_in(&url_bar, window, |this, input, e, _, cx| {
            if matches!(e, InputEvent::PressEnter { .. }) {
                let typed = input.read(cx).value();
                if let Err(e) = this.open_from_url_bar(&typed, cx) {
                    this.error = Some(format!("{e:#}").into());
                }
                cx.notify();
            }
        });
        // Only full loads report here; pushState and hash changes leave the bar as it was.
        let sync_url_bar = cx.spawn_in(window, async move |this, cx| {
            while let Ok(url) = loaded_rx.recv_async().await {
                let updated = this.update_in(cx, |this, window, cx| {
                    this.loaded = true;
                    this.loads += 1;
                    this.committed_url = Some(url.clone());
                    this.page_changed();
                    if this.load_error.take().is_some() {
                        this.retrying = false;
                        this.apply_visibility(cx);
                        cx.notify();
                    }
                    this.url_bar
                        .update(cx, |bar, cx| bar.set_value(url, window, cx));
                });
                if updated.is_err() {
                    break;
                }
            }
        });

        Self {
            _live: skyelab_core::debug::Live::new("browser tabs"),
            webview,
            engine,
            url_bar,
            error: error.map(|e| format!("{e:#}").into()),
            loaded: false,
            asked: false,
            probe_local: false,
            loads: 0,
            committed_url: None,
            title: None,
            load_error: None,
            retrying: false,
            load_failures: 0,
            #[cfg(target_os = "macos")]
            _failures: None,
            _failed: Task::ready(()),
            focus: cx.focus_handle(),
            zoom: 1.0,
            shown: true,
            menu_open: false,
            private: false,
            agent_page: false,
            find: None,
            _find_task: Task::ready(()),
            _probe: Task::ready(()),
            _data_dir: None,
            _subscription: subscription,
            _sync_url_bar: sync_url_bar,
            _popups: Task::ready(()),
            _sync_title: Task::ready(()),
        }
    }

    fn page<'a>(&self, cx: &'a gpui::App) -> anyhow::Result<&'a WebView> {
        Ok(self
            .webview
            .as_ref()
            .context("no webview in this window")?
            .read(cx))
    }

    /// The agent path: loads `input` after [`parse_agent_url`] accepts it (http or https only).
    /// Rejected input never reaches the webview.
    pub fn navigate(&mut self, input: &str, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let url = parse_agent_url(input)?;
        self.load(&url, cx)
    }

    fn open_from_url_bar(&mut self, input: &str, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let url = parse_url(input)?;
        self.load(&url, cx)
    }

    fn engine(&self) -> anyhow::Result<&dyn Engine> {
        self.engine.as_deref().context("no webview in this window")
    }

    pub(crate) fn load(&mut self, url: &url::Url, cx: &mut Context<Self>) -> anyhow::Result<()> {
        self.engine()?;
        if !self.asked {
            self.asked = true;
            self.apply_visibility(cx);
        }
        // Nothing listening on a local port: say so on every OS, rather than a blank page (macOS) or the engine's own
        // error page (Linux, Windows).
        // The connect blocks (up to 250 ms an address), so it runs in the background and the load follows it.
        if self.probe_local
            && let Some(addrs) = crate::load_error::local_addrs(url)
        {
            let url = url.clone();
            let probe = {
                let url = url.clone();
                cx.background_spawn(async move { crate::load_error::refused_at(&addrs, &url) })
            };
            // A newer load replaces this task, which drops (cancels) this one's follow-up.
            self._probe = cx.spawn(async move |this, cx| {
                let refused = probe.await;
                let _ = this.update(cx, |this, cx| {
                    this.error = None;
                    match refused {
                        Some(refused) => this.load_failed(Some(refused), cx),
                        None => {
                            let loaded = this.engine().and_then(|e| e.load(url.as_str(), cx));
                            this.report(loaded, cx);
                        }
                    }
                });
            });
            return Ok(());
        }
        self._probe = Task::ready(());
        self.engine()?.load(url.as_str(), cx)?;
        self.error = None;
        cx.notify();
        Ok(())
    }

    /// Shows or hides the native view, which otherwise stays on screen when its pane isn't drawn.
    pub fn set_visible(&mut self, visible: bool, cx: &gpui::App) {
        self.shown = visible;
        // A pane that isn't drawn drops its open menu without a close event.
        if !visible {
            self.menu_open = false;
        }
        self.apply_visibility(cx);
    }

    /// The native view draws over GPUI overlays, so it is hidden while the ⋯ menu is open over it, and while a
    /// failed load's error shows in its place.
    fn apply_visibility(&self, cx: &gpui::App) {
        if let Some(page) = &self.webview {
            let _ = page.read(cx).set_visible(
                self.shown && !self.menu_open && self.load_error.is_none() && self.asked,
            );
        }
    }

    /// Hides and drops the native webview, for when the pane closes.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.set_visible(false, cx);
        self.webview = None;
        self.engine = None;
        self.loaded = false;
        cx.notify();
    }

    /// How many pages have committed since the pane opened; one more after a load the caller started.
    pub fn load_count(&self) -> u64 {
        self.loads
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        if self.dismiss_load_error(cx) {
            return;
        }
        let result = self
            .page(cx)
            .and_then(|p| Ok(p.evaluate_script("history.back()")?));
        self.report(result, cx);
    }

    fn forward(&mut self, cx: &mut Context<Self>) {
        if self.load_error.is_some() {
            return; // the history is the page underneath's: forward from it isn't forward from here
        }
        let result = self
            .page(cx)
            .and_then(|p| Ok(p.evaluate_script("history.forward()")?));
        self.report(result, cx);
    }

    /// Reloads the page, which also ends every script running in it.
    /// On the error page: loads the failed address again ([`Self::retry`]).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if self.load_error.is_some() {
            return self.retry(cx);
        }
        let result = self.engine().and_then(|e| e.reload(cx));
        self.report(result, cx);
    }

    fn report(&mut self, result: anyhow::Result<()>, cx: &mut Context<Self>) {
        self.error = result.err().map(|e| format!("{e:#}").into());
        cx.notify();
    }
}

impl Focusable for BrowserView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for BrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let nav =
            |id: &'static str, label: &'static str| Button::new(id).label(label).small().ghost();
        let toolbar = div()
            .flex()
            .items_center()
            .gap_1()
            .p_1()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .debug_selector(|| "browser-back".into())
                    .child(nav("back", "←").on_click(cx.listener(|this, _, _, cx| this.back(cx)))),
            )
            .child(nav("forward", "→").on_click(cx.listener(|this, _, _, cx| this.forward(cx))))
            .child(nav("reload", "↻").on_click(cx.listener(|this, _, _, cx| this.reload(cx))))
            .child(div().flex_1().child(Input::new(&self.url_bar).small()))
            .children(self.render_zoom_chip(cx))
            .child(self.render_menu(cx));
        let find_bar = self.render_find(window, cx);
        let theme = cx.theme();
        div()
            .track_focus(&self.focus)
            .key_context(controls::CONTEXT)
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::zoom_reset))
            .on_action(cx.listener(Self::open_find))
            .on_action(cx.listener(Self::close_find))
            .on_action(cx.listener(|this, _: &controls::BrowserReload, _, cx| this.reload(cx)))
            .on_action(cx.listener(Self::duplicate))
            .on_action(cx.listener(Self::copy_url))
            .on_action(cx.listener(Self::open_external))
            .when(cfg!(debug_assertions), |d| {
                d.on_action(cx.listener(Self::dev_tools))
            })
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .child(toolbar)
            // A row, not an overlay: the page would draw over an overlay.
            .children(find_bar)
            .children(self.error.clone().map(|e| {
                div()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(theme.danger)
                    .child(e)
            }))
            // Not drawn while the error shows: drawing the page's element would put the native view back on top.
            .map(|d| match &self.load_error {
                Some(e) => d.child(self.render_load_error(e, cx)),
                None if !self.asked => d.child(self.render_no_page(cx)),
                None => d.children(self.webview.clone().map(|w| div().flex_1().child(w))),
            })
    }
}

#[path = "view_agent.rs"]
mod agent_ui;
#[path = "view_controls.rs"]
mod controls_ui;
#[path = "view_error.rs"]
mod error_ui;
#[path = "view_wry.rs"]
mod wry_ui;
pub use controls_ui::DuplicateTab;

/// `document.title` as wry hands it back (a JSON string), trimmed, with control characters (newlines, bidi
/// overrides) dropped so a page can't break or disguise its tab's label. `None` when empty or not a string.
fn page_title(raw: &str) -> Option<SharedString> {
    let title: String = serde_json::from_str::<String>(raw)
        .ok()?
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .collect();
    let title = title.trim();
    (!title.is_empty()).then(|| SharedString::from(title.to_owned()))
}

#[cfg(test)]
#[test]
fn page_titles_are_cleaned() {
    assert_eq!(page_title("\"QA page\"").as_deref(), Some("QA page"));
    assert_eq!(page_title("\"  \""), None);
    assert_eq!(page_title("null"), None, "no title");
    assert_eq!(page_title("\"a\\nb\\u202ec\"").as_deref(), Some("abc"));
}
