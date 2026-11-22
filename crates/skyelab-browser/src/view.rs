use std::{future::Future, sync::Mutex};

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

use crate::{js, parse_url};

/// A browser pane. Agents drive it with `navigate`, `current_url` and `eval_js`.
pub struct BrowserView {
    /// `None` when the native webview couldn't be built (e.g. a Wayland window); `error` says why.
    webview: Option<Entity<WebView>>,
    url_bar: Entity<InputState>,
    error: Option<SharedString>,
    focus: FocusHandle,
    _subscription: Subscription,
    _sync_url_bar: Task<()>,
}

impl BrowserView {
    /// Opens `url` (validated with [`parse_url`]) in a new native webview attached to `window`.
    /// A rejected URL or a webview that can't be built shows as an error in the pane.
    pub fn new(url: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (loaded_tx, loaded_rx) = flume::unbounded::<String>();
        let mut builder =
            wry::WebViewBuilder::new().with_on_page_load_handler(move |event, url| {
                if matches!(event, wry::PageLoadEvent::Started) {
                    let _ = loaded_tx.send(url);
                }
            });
        let mut error = None;
        match parse_url(url) {
            Ok(url) => builder = builder.with_url(url.as_str()),
            Err(e) => error = Some(e),
        }
        let webview = match WebView::build(builder, window, cx).context("create the webview") {
            Ok(webview) => Some(cx.new(|_| webview)),
            Err(e) => {
                error = Some(e);
                None
            }
        };

        let url_bar = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("localhost:3000")
                .default_value(url.trim().to_string())
        });
        let subscription = cx.subscribe_in(&url_bar, window, |this, input, e, _, cx| {
            if matches!(e, InputEvent::PressEnter { .. }) {
                let typed = input.read(cx).value();
                if let Err(e) = this.navigate(&typed, cx) {
                    this.error = Some(format!("{e:#}").into());
                }
                cx.notify();
            }
        });
        // Only full loads report here; pushState and hash changes leave the bar as it was.
        let sync_url_bar = cx.spawn_in(window, async move |this, cx| {
            while let Ok(url) = loaded_rx.recv_async().await {
                let updated = this.update_in(cx, |this, window, cx| {
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
            url_bar,
            error: error.map(|e| format!("{e:#}").into()),
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

    /// Loads `input` after [`parse_url`] accepts it. Rejected input never reaches the webview.
    pub fn navigate(&mut self, input: &str, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let url = parse_url(input)?;
        self.page(cx)?.raw().load_url(url.as_str())?;
        self.error = None;
        cx.notify();
        Ok(())
    }

    /// The URL the page is on now, as the engine reports it.
    pub fn current_url(&self, cx: &gpui::App) -> anyhow::Result<String> {
        Ok(self.page(cx)?.url()?)
    }

    /// Evaluates a JS *expression* in the page and returns its JSON value. A thrown exception is an
    /// `Err` with its message. Values JSON can't hold (DOM nodes, functions) come back as `{}` or `null`;
    /// a Promise is not awaited.
    pub fn eval_js(
        &self,
        expression: &str,
        cx: &gpui::App,
    ) -> impl Future<Output = anyhow::Result<serde_json::Value>> + use<> {
        let (tx, rx) = oneshot::channel();
        let tx = Mutex::new(Some(tx));
        let started = self.page(cx).and_then(|page| {
            page.evaluate_script_with_callback(&js::wrap(expression), move |raw| {
                if let Some(tx) = tx.lock().ok().and_then(|mut tx| tx.take()) {
                    let _ = tx.send(raw);
                }
            })
            .context("evaluate script")
        });
        async move {
            started?;
            // Before the first load, wry queues the script and drops the callback, cancelling `rx`.
            let raw = rx
                .await
                .context("page not loaded yet: the script was queued and runs on the next load")?;
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
