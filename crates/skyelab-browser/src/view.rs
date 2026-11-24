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
    controls,
    engine::Wry,
    js, parse_url,
};

/// A browser pane. Agents drive it with `navigate`, `current_url` and `eval_js`, which only reach
/// http and https pages. The URL bar is the user's path and also opens local files.
pub struct BrowserView {
    /// `None` when the native webview couldn't be built (e.g. a Wayland window); `error` says why.
    webview: Option<Entity<WebView>>,
    /// What loads and runs scripts: the webview, or a test's fake. `None` once closed.
    engine: Option<Rc<dyn Engine>>,
    url_bar: Entity<InputState>,
    error: Option<SharedString>,
    /// A page has committed (wry's `Started`). Until then wry queues scripts for the next page.
    loaded: bool,
    /// Pages committed so far, so a caller can wait for the next one.
    loads: u64,
    /// The URL of the page that committed last (wry's `Started`), which the user sees. The engine's own URL can
    /// name a pending load.
    committed_url: Option<String>,
    focus: FocusHandle,
    zoom: f64,
    /// The host shows the pane ([`Self::set_visible`]); the page is also hidden while the ⋯ menu is open.
    shown: bool,
    menu_open: bool,
    find: Option<controls_ui::Find>,
    _find_task: Task<()>,
    _data_dir: Option<tempfile::TempDir>,
    _subscription: Subscription,
    _sync_url_bar: Task<()>,
}

impl BrowserView {
    /// Opens `url` (the user's, validated with [`parse_url`]), or a blank page for `None`, in a new
    /// native webview attached to `window`. A rejected URL or a webview that can't be built shows
    /// as an error in the pane.
    pub fn new(url: Option<&str>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_wry(url, false, window, cx)
    }

    /// A blank pane for an agent, on a private, in-memory web data store of its own: no cookies,
    /// storage or cache shared with the user's tabs, other agents' tabs or later runs.
    pub fn new_agent(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_wry(None, true, window, cx)
    }

    fn with_wry(
        url: Option<&str>,
        private: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (loaded_tx, loaded_rx) = flume::unbounded::<String>();
        let mut no_folder = None;
        let override_env = std::env::var_os("WEBVIEW2_USER_DATA_FOLDER");
        let data_dir = crate::webview_data_dir(cfg!(windows), private, override_env.as_deref())
            .unwrap_or_else(|e| {
                no_folder = Some(e.context("give the agent tab its own web data folder"));
                None
            });
        let mut context = data_dir
            .as_ref()
            .map(|d| wry::WebContext::new(Some(d.path().to_owned())));
        let builder = match context.as_mut() {
            Some(context) => wry::WebViewBuilder::new_with_web_context(context),
            None => wry::WebViewBuilder::new(),
        };
        let mut builder =
            builder
                .with_incognito(private)
                .with_on_page_load_handler(move |event, url| {
                    if matches!(event, wry::PageLoadEvent::Started) {
                        let _ = loaded_tx.send(url);
                    }
                });
        let mut error = None;
        match url.map(parse_url) {
            Some(Ok(url)) => builder = builder.with_url(url.as_str()),
            Some(Err(e)) => error = Some(e),
            None => {}
        }
        // A private tab whose own folder couldn't be made gets no webview rather than a shared session.
        let webview = match no_folder {
            Some(e) => {
                error = Some(e);
                None
            }
            None => match WebView::build(builder, window, cx).context("create the webview") {
                Ok(webview) => Some(cx.new(|_| webview)),
                Err(e) => {
                    error = Some(e);
                    None
                }
            },
        };
        let engine = webview.clone().map(|w| Rc::new(Wry(w)) as Rc<dyn Engine>);
        let mut view = Self::build(webview, engine, loaded_rx, url, error, window, cx);
        view._data_dir = data_dir;
        view
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
                    this.url_bar
                        .update(cx, |bar, cx| bar.set_value(url, window, cx));
                });
                if updated.is_err() {
                    break;
                }
            }
        });

        Self {
            webview,
            engine,
            url_bar,
            error: error.map(|e| format!("{e:#}").into()),
            loaded: false,
            loads: 0,
            committed_url: None,
            focus: cx.focus_handle(),
            zoom: 1.0,
            shown: true,
            menu_open: false,
            find: None,
            _find_task: Task::ready(()),
            _data_dir: None,
            _subscription: subscription,
            _sync_url_bar: sync_url_bar,
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

    fn load(&mut self, url: &url::Url, cx: &mut Context<Self>) -> anyhow::Result<()> {
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

    /// The native view draws over GPUI overlays, so it is hidden while the ⋯ menu is open over it.
    fn apply_visibility(&self, cx: &gpui::App) {
        if let Some(page) = &self.webview {
            let _ = page.read(cx).set_visible(self.shown && !self.menu_open);
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

    /// The page for an agent call, and its URL as the engine reports it (which may be a pending load).
    /// Refused before the first load (wry panics on the nil URL then) and on non-http(s) pages.
    fn agent_page(&self, cx: &gpui::App) -> anyhow::Result<(&dyn Engine, String)> {
        let url = match self.loaded {
            true => self.engine()?.url(cx)?,
            false => String::new(),
        };
        check_agent_eval(&url, self.loaded)?;
        Ok((self.engine()?, url))
    }

    /// The URL the page is on now, as the engine reports it. Refused for local files, so an agent
    /// doesn't learn the user's paths.
    pub fn current_url(&self, cx: &gpui::App) -> anyhow::Result<String> {
        Ok(self.agent_page(cx)?.1)
    }

    /// Evaluates a JS *expression* in the page and returns its JSON value. Refused before the first
    /// load and in any page that isn't http or https, checked again inside the page itself. A thrown
    /// exception is an `Err` with its message. Values JSON can't hold (DOM nodes, functions) come
    /// back as `{}` or `null`; a Promise is not awaited.
    pub fn eval_js(
        &self,
        expression: &str,
        cx: &gpui::App,
    ) -> impl Future<Output = anyhow::Result<serde_json::Value>> + use<> {
        let (tx, rx) = oneshot::channel();
        let started = self.agent_page(cx).and_then(|(page, _)| {
            page.eval(
                &js::wrap(expression),
                Box::new(move |raw| _ = tx.send(raw)),
                cx,
            )
        });
        async move {
            started?;
            // wry drops the callback of a script it queued for the next load.
            let raw = rx
                .await
                .context("the webview queued the script instead of running it")?;
            js::decode(&raw)
        }
    }

    /// What the page shows now, as the engine's encoded image (see [`crate::encode_png`] for the PNG the agent
    /// gets). Refused before the first load and unless the painted document itself is http or https and parsed:
    /// the engine URL can name a pending load while the old page, maybe a local file, is still on screen. An image
    /// taken while a new page committed is dropped.
    pub fn screenshot(
        this: &Entity<Self>,
        cx: &mut gpui::App,
    ) -> impl Future<Output = anyhow::Result<Vec<u8>>> + use<> {
        let this = this.clone();
        let cx = cx.to_async();
        async move {
            // The in-page guard of eval_js checks `location` in the document that is actually there.
            let (state, before) = cx.read_entity(&this, |b, cx| {
                (b.eval_js("document.readyState", cx), b.loads)
            });
            if state.await? == "loading" {
                anyhow::bail!("the page is still loading; try again in a moment");
            }
            let (tx, rx) = oneshot::channel();
            cx.read_entity(&this, |b, cx| {
                let (page, _) = b.agent_page(cx)?;
                page.snapshot(Box::new(move |r| _ = tx.send(r)), cx)
            })?;
            let raw = rx
                .await
                .context("the webview dropped the snapshot")?
                .map_err(|e| anyhow::anyhow!("snapshot failed: {e}"))?;
            if cx.read_entity(&this, |b, _| b.loads) != before {
                anyhow::bail!("a new page loaded while the screenshot was taken; try again");
            }
            Ok(raw)
        }
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        let result = self
            .page(cx)
            .and_then(|p| Ok(p.evaluate_script("history.back()")?));
        self.report(result, cx);
    }

    fn forward(&mut self, cx: &mut Context<Self>) {
        let result = self
            .page(cx)
            .and_then(|p| Ok(p.evaluate_script("history.forward()")?));
        self.report(result, cx);
    }

    /// Reloads the page, which also ends every script running in it.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
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
            .child(nav("back", "←").on_click(cx.listener(|this, _, _, cx| this.back(cx))))
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
            .children(self.webview.clone().map(|w| div().flex_1().child(w)))
    }
}

#[path = "view_controls.rs"]
mod controls_ui;
pub use controls_ui::DuplicateTab;
