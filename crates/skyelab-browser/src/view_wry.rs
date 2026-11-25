//! Building a pane on a real wry page: its web data folder, load and popup channels, the failure watcher and the
//! title sync, then its first page.

use std::rc::Rc;

use anyhow::Context as _;
use gpui::{AppContext as _, Context, Window};
use gpui_webview::WebView;

use super::{BrowserView, DuplicateTab, page_title};
use crate::{Engine, engine::Wry, parse_url};

impl BrowserView {
    pub(super) fn with_wry(
        url: Option<&str>,
        private: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (loaded_tx, loaded_rx) = flume::unbounded::<String>();
        let (finished_tx, finished_rx) = flume::unbounded::<()>();
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
        let mut builder = builder.with_incognito(private).with_on_page_load_handler(
            move |event, url| match event {
                wry::PageLoadEvent::Started => _ = loaded_tx.send(url),
                wry::PageLoadEvent::Finished => _ = finished_tx.send(()),
            },
        );
        // A private page goes nowhere but the web, saves no download, and its popups come back as private tabs.
        let (popup_tx, popups) = flume::unbounded::<String>();
        if private {
            builder = builder
                .with_navigation_handler(|url| crate::controls::private_navigation_allowed(&url))
                .with_new_window_req_handler(move |url, _| {
                    if let Some(url) = crate::controls::popup_tab(&url) {
                        let _ = popup_tx.send(url);
                    }
                    wry::NewWindowResponse::Deny
                })
                .with_download_started_handler(|_, _| false);
        }
        // The first page loads once the pane is built, through `load`: its failure watcher is on by then, and a
        // dead local address shows the error page rather than a blank one.
        let mut error = None;
        let first = match url.map(parse_url) {
            Some(Ok(url)) => Some(url),
            Some(Err(e)) => {
                error = Some(e);
                None
            }
            None => None,
        };
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
        view.private = private;
        view._popups = cx.spawn(async move |this, cx| {
            while let Ok(url) = popups.recv_async().await {
                let open = this.update(cx, |_, cx| cx.emit(DuplicateTab(Some(url))));
                if open.is_err() {
                    return;
                }
            }
        });
        #[cfg(target_os = "macos")]
        view.watch_failures(cx);
        view.probe_local = true;
        if let Some(first) = first
            && let Err(e) = view.load(&first, cx)
        {
            view.error = Some(format!("{e:#}").into());
        }
        // Asked after each load, not through wry's title-changed observer: lb-wry 0.53 dereferences WebKit's
        // nullable title there. A title a page sets later shows from its next load on.
        let page = view.webview.clone();
        view._sync_title = cx.spawn(async move |this, cx| {
            let Some(page) = page else { return };
            while finished_rx.recv_async().await.is_ok() {
                let (tx, rx) = flume::bounded::<String>(1);
                let asked = cx.update(|cx| {
                    page.read(cx)
                        .evaluate_script_with_callback("document.title", move |raw| {
                            let _ = tx.try_send(raw);
                        })
                        .is_ok()
                });
                if !asked {
                    continue;
                }
                let Ok(raw) = rx.recv_async().await else {
                    continue;
                };
                let title = page_title(&raw);
                let shown = this.update(cx, |this, cx| {
                    this.title = title;
                    cx.notify();
                });
                if shown.is_err() {
                    break;
                }
            }
        });
        view
    }
}
