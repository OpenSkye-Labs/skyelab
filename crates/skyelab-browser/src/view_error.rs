//! The pane's "This site can't be reached" page, drawn by GPUI where the page would be. The native page is hidden
//! while it shows: it would draw over anything in its bounds.

use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement as _, IntoElement as _, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _,
    px,
};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
};

use super::BrowserView;
use crate::load_error::LoadError;

/// The address the empty tab offers (the bar's placeholder too).
const LOCAL_DEV: &str = "localhost:3000";

impl BrowserView {
    /// A load that failed: the page is hidden and [`Self::render_load_error`] shows instead, until the next page
    /// commits. `None` (a cancelled load, a download) changes nothing but ending a Retry's spinner.
    pub fn load_failed(&mut self, error: Option<LoadError>, cx: &mut Context<Self>) {
        self.retrying = false;
        let Some(error) = error else {
            return cx.notify();
        };
        self.load_error = Some(error);
        self.load_failures += 1;
        self.apply_visibility(cx);
        cx.notify();
    }

    /// Turns WebKit's failed loads into [`Self::load_failed`]. Linux's and Windows' engines draw an error page of
    /// their own, which commits like any page.
    #[cfg(target_os = "macos")]
    pub(super) fn watch_failures(&mut self, cx: &mut Context<Self>) {
        use wry::WebViewExtMacOS as _;
        let Some(page) = &self.webview else { return };
        let (tx, failures) = flume::unbounded::<skyelab_webkit_sys::LoadFailure>();
        self._failures =
            skyelab_webkit_sys::watch_load_failures(&page.read(cx).raw().webview(), move |f| {
                let _ = tx.send(f);
            });
        self._failed = cx.spawn(async move |this, cx| {
            while let Ok(f) = failures.recv_async().await {
                let error = crate::load_error::describe(
                    &f.domain,
                    f.code,
                    &f.description,
                    f.url.as_deref().unwrap_or_default(),
                );
                let keeps = crate::load_error::keeps_page(&f.domain, f.code);
                let shown = this.update(cx, |this, cx| {
                    let error = error.filter(|_| !(keeps && this.loaded));
                    this.load_failed(error, cx)
                });
                if shown.is_err() {
                    return;
                }
            }
        });
    }

    /// The failed page's error, if one shows.
    pub fn load_error(&self) -> Option<&LoadError> {
        self.load_error.as_ref()
    }

    /// How many loads have failed since the pane opened, so a caller can tell a new failure from an old one.
    pub fn load_failures(&self) -> u64 {
        self.load_failures
    }

    /// Back from the error page: the page before it is still loaded underneath, so show it again.
    pub(super) fn dismiss_load_error(&mut self, cx: &mut Context<Self>) -> bool {
        if self.load_error.is_none() || !self.loaded {
            return false;
        }
        self.load_error = None;
        self.retrying = false;
        self.apply_visibility(cx);
        cx.notify();
        true
    }

    /// Loads the failed address again; the error stays until a page commits or the load fails again.
    pub fn retry(&mut self, cx: &mut Context<Self>) {
        let Some(failed) = self.load_error.as_ref().map(|e| e.url.clone()) else {
            return;
        };
        // WebKit nearly always names the failed address; else the bar's (what was asked for), else the last page.
        let typed = self.url_bar.read(cx).value().trim().to_owned();
        let url = [
            failed,
            typed,
            self.committed_url.clone().unwrap_or_default(),
        ]
        .into_iter()
        .find(|u| !u.is_empty());
        let Some(url) = url else { return };
        // A refused local address fails inside `load`, which ends the spinner.
        self.retrying = true;
        let result = crate::parse_url(&url).and_then(|url| self.load(&url, cx));
        if result.is_err() {
            self.retrying = false;
        }
        self.report(result, cx);
    }

    /// A tab nothing was loaded in yet (T3's "No preview yet"): what to type, and a one-click local dev server.
    pub(super) fn render_no_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        div()
            .id("no-page")
            .debug_selector(|| "browser-no-page".into())
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .px_8()
            .bg(theme.background)
            .child(
                gpui_component::Icon::new(IconName::Globe)
                    .size(px(24.))
                    .text_color(theme.muted_foreground),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .child("No page yet"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .text_center()
                    .child("Type an address above, or open your dev server."),
            )
            .child(
                div()
                    .mt_2()
                    .debug_selector(|| "browser-open-local".into())
                    .child(
                        Button::new("no-page-open-local")
                            .label(format!("Open {LOCAL_DEV}"))
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.url_bar
                                    .update(cx, |bar, cx| bar.set_value(LOCAL_DEV, window, cx));
                                if let Err(e) = this.open_from_url_bar(LOCAL_DEV, cx) {
                                    this.error = Some(format!("{e:#}").into());
                                }
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_load_error(
        &self,
        error: &LoadError,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let text = |s: SharedString| div().child(s);
        div()
            .id("load-error")
            .debug_selector(|| "browser-load-error".into())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .bg(theme.background)
            .child(
                div()
                    .mx_auto()
                    .max_w(px(560.))
                    .px_8()
                    .py_12()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        gpui_component::Icon::new(IconName::TriangleAlert)
                            .size(px(40.))
                            .text_color(theme.muted_foreground.opacity(0.7)),
                    )
                    .child(
                        div()
                            .mt_3()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child(error.title.clone()),
                    )
                    .child(
                        div().text_sm().text_color(theme.muted_foreground).child(
                            div()
                                .flex()
                                .flex_wrap()
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.foreground)
                                        .child(error.host.clone()),
                                )
                                .child(text(format!(": {}.", error.reason).into())),
                        ),
                    )
                    .when_some(error.hint.clone(), |d, hint| {
                        d.child(div().text_sm().text_color(theme.foreground).child(hint))
                    })
                    .child(
                        div()
                            .mt_4()
                            .text_xs()
                            .text_color(theme.muted_foreground.opacity(0.8))
                            .child(error.detail.clone()),
                    )
                    .child(
                        div()
                            .mt_6()
                            .flex()
                            // On the button's own box: the row is as wide as the page.
                            .child(
                                div().debug_selector(|| "browser-load-retry".into()).child(
                                    Button::new("load-error-retry")
                                        .label("Retry")
                                        .icon(IconName::Redo2)
                                        .small()
                                        .primary()
                                        .loading(self.retrying)
                                        .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                                ),
                            ),
                    ),
            )
            .into_any_element()
    }
}
