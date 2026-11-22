use std::{future::Future, rc::Rc};

use anyhow::Context as _;
use futures::channel::oneshot;
use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Task, Window, div,
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
    engine::Wry,
    js, parse_url,
};

/// A browser pane. Agents drive it with `navigate`, `current_url` and `eval_js`, which only reach
/// http and https pages. The URL bar is the user's path and also opens local files.
pub struct BrowserView {
    /// `None` when the native webview couldn't be built (e.g. a Wayland window); `error` says why.
    webview: Option<Entity<WebView>>,
    /// What loads and runs scripts: the webview, or a test's fake. `None` with `webview`.
    engine: Option<Rc<dyn Engine>>,
    url_bar: Entity<InputState>,
    error: Option<SharedString>,
    /// A page has committed (wry's `Started`). Until then wry queues scripts for the next page.
    loaded: bool,
    focus: FocusHandle,
    _subscription: Subscription,
    _sync_url_bar: Task<()>,
}

impl BrowserView {
    /// Opens `url` (the user's, validated with [`parse_url`]), or a blank page for `None`, in a new
    /// native webview attached to `window`. A rejected URL or a webview that can't be built shows
    /// as an error in the pane.
    pub fn new(url: Option<&str>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (loaded_tx, loaded_rx) = flume::unbounded::<String>();
        let mut builder =
            wry::WebViewBuilder::new().with_on_page_load_handler(move |event, url| {
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
        let webview = match WebView::build(builder, window, cx).context("create the webview") {
            Ok(webview) => Some(cx.new(|_| webview)),
            Err(e) => {
                error = Some(e);
                None
            }
        };
        let engine = webview.clone().map(|w| Rc::new(Wry(w)) as Rc<dyn Engine>);
        Self::build(webview, engine, loaded_rx, url, error, window, cx)
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
            focus: cx.focus_handle(),
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
    pub fn set_visible(&self, visible: bool, cx: &gpui::App) {
        if let Some(page) = &self.webview {
            let _ = page.read(cx).set_visible(visible);
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

    /// A page has committed since the pane opened. Before that, agent scripts are refused.
    pub fn is_loaded(&self) -> bool {
        self.loaded
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

    fn reload(&mut self, cx: &mut Context<Self>) {
        let result = self.page(cx).and_then(|p| Ok(p.reload()?));
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let nav =
            |id: &'static str, label: &'static str| Button::new(id).label(label).small().ghost();
        div()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .p_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(nav("back", "←").on_click(cx.listener(|this, _, _, cx| this.back(cx))))
                    .child(
                        nav("forward", "→")
                            .on_click(cx.listener(|this, _, _, cx| this.forward(cx))),
                    )
                    .child(
                        nav("reload", "↻").on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                    )
                    .child(div().flex_1().child(Input::new(&self.url_bar).small())),
            )
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
