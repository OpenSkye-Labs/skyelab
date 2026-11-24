//! The tab's own commands: zoom, find in page, and the ⋯ menu (Reload, Duplicate, Copy URL, Open in system
//! browser, DevTools).

use futures::channel::oneshot;
use gpui::{
    AnyElement, AppContext as _, ClipboardItem, Context, Entity, EventEmitter, FocusHandle,
    InteractiveElement as _, IntoElement as _, ParentElement as _, Styled as _, Subscription,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::DropdownMenu as _,
};

use super::BrowserView;
use crate::controls::{self, *};

/// Duplicate tab: the host opens a user tab on this URL (`None`: a blank one), never an agent tab.
pub struct DuplicateTab(pub Option<String>);

impl EventEmitter<DuplicateTab> for BrowserView {}

/// The open find bar.
pub(super) struct Find {
    input: Entity<InputState>,
    /// The last search found nothing.
    missed: bool,
    /// The query last searched: a Change that leaves it as it was (Enter sends one) doesn't start over.
    searched: String,
    _sub: Subscription,
}

impl BrowserView {
    /// The page zoom, 1.0 being the page's own size.
    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    fn set_zoom(&mut self, zoom: f64, cx: &mut Context<Self>) {
        self.zoom = zoom;
        let result = match &self.webview {
            Some(page) => page.read(cx).set_zoom(zoom).map_err(Into::into),
            None => Ok(()),
        };
        self.report(result, cx);
    }

    pub(super) fn zoom_in(&mut self, _: &BrowserZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom(controls::zoom_step(self.zoom, true), cx);
    }

    pub(super) fn zoom_out(&mut self, _: &BrowserZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom(controls::zoom_step(self.zoom, false), cx);
    }

    pub(super) fn zoom_reset(
        &mut self,
        _: &BrowserZoomReset,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_zoom(1.0, cx);
    }

    /// The committed page's URL. `None` before the first load, when wry would panic on the nil URL.
    fn page_url(&self, cx: &gpui::App) -> Option<String> {
        self.loaded
            .then(|| self.engine().ok()?.url(cx).ok())
            .flatten()
    }

    /// What the URL bar shows.
    pub fn url_bar_text(&self, cx: &gpui::App) -> String {
        self.url_bar.read(cx).value().to_string()
    }

    pub(super) fn duplicate(
        &mut self,
        _: &BrowserDuplicate,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(DuplicateTab(self.page_url(cx)));
    }

    pub(super) fn copy_url(&mut self, _: &BrowserCopyUrl, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(url) = self.page_url(cx) {
            cx.write_to_clipboard(ClipboardItem::new_string(url));
        }
    }

    pub(super) fn open_external(
        &mut self,
        _: &BrowserOpenExternal,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self
            .page_url(cx)
            .as_deref()
            .and_then(controls::external_url)
        {
            Some(url) => cx.open_url(url.as_str()),
            None => self.report(
                Err(anyhow::anyhow!(
                    "only http and https pages open in your browser"
                )),
                cx,
            ),
        }
    }

    pub(super) fn dev_tools(
        &mut self,
        _: &BrowserDevTools,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // wry has DevTools only in debug builds (its `devtools` feature calls private API on macOS).
        #[cfg(debug_assertions)]
        if let Some(page) = &self.webview {
            page.read(cx).raw().open_devtools();
        }
        let _ = cx;
    }

    fn set_menu_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.menu_open = open;
        self.apply_visibility(cx);
        cx.notify();
    }

    pub(super) fn open_find(
        &mut self,
        _: &BrowserFind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.find.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in page"));
            let sub = cx.subscribe(&input, |this, _, e: &InputEvent, cx| match e {
                InputEvent::Change => {
                    if this.find_query(cx) != this.find.as_ref().map(|f| f.searched.clone()) {
                        this.run_find(false, true, cx)
                    }
                }
                InputEvent::PressEnter { .. } => this.run_find(false, false, cx),
                _ => {}
            });
            self.find = Some(Find {
                input,
                missed: false,
                searched: String::new(),
                _sub: sub,
            });
        }
        if let Some(find) = &self.find {
            find.input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        }
        cx.notify();
    }

    pub(super) fn close_find(
        &mut self,
        _: &BrowserCloseFind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.find.take().is_some() {
            if let Ok(engine) = self.engine() {
                let _ = engine.find("", false, false, Box::new(|_| {}), cx);
            }
            self.focus.focus(window, cx);
            cx.notify();
        }
    }

    /// The find bar's query, while it is open.
    pub fn find_query(&self, cx: &gpui::App) -> Option<String> {
        Some(self.find.as_ref()?.input.read(cx).value().to_string())
    }

    /// The last search found nothing.
    pub fn find_missed(&self) -> bool {
        self.find.as_ref().is_some_and(|f| f.missed)
    }

    /// Types `query` into the open find bar and searches for it.
    pub fn set_find_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(find) = &self.find else { return };
        find.input.update(cx, |input, cx| {
            input.set_value(query.to_owned(), window, cx)
        });
        // set_value emits no Change.
        self.run_find(false, true, cx);
    }

    /// Searches the page for the bar's query; an empty query clears the last match.
    fn run_find(&mut self, backwards: bool, restart: bool, cx: &mut Context<Self>) {
        let Some(query) = self.find_query(cx) else {
            return;
        };
        if let Some(find) = &mut self.find {
            find.searched = query.clone();
        }
        let (tx, rx) = oneshot::channel();
        let started = self.engine().and_then(|engine| {
            engine.find(
                &query,
                backwards,
                restart,
                Box::new(move |hit| _ = tx.send(hit)),
                cx,
            )
        });
        if let Err(e) = started {
            return self.report(Err(e), cx);
        }
        // A newer search replaces this task, so only the latest answer lands.
        self._find_task = cx.spawn(async move |this, cx| {
            let hit = rx.await.unwrap_or(false);
            let _ = this.update(cx, |this, cx| {
                if let Some(find) = &mut this.find {
                    find.missed = !hit && !query.is_empty();
                    cx.notify();
                }
            });
        });
    }

    /// The zoom level, when the page isn't at its own size; clicking it resets.
    pub(super) fn render_zoom_chip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        ((self.zoom - 1.0).abs() > 1e-6).then(|| {
            Button::new("zoom")
                .label(format!("{:.0}%", self.zoom * 100.0))
                .small()
                .ghost()
                .tooltip("Reset zoom")
                .on_click(cx.listener(|this, _, _, cx| this.set_zoom(1.0, cx)))
                .into_any_element()
        })
    }

    /// The ⋯ menu. The page hides while it is open: it would draw over the menu.
    pub(super) fn render_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        let focus: FocusHandle = self.focus.clone();
        let reopen = this.clone();
        Button::new("browser-menu")
            .icon(IconName::Ellipsis)
            .small()
            .ghost()
            .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, cx| {
                let url = this.upgrade().and_then(|b| b.read(cx).page_url(cx));
                let no_page = url.is_none();
                let no_web = url.as_deref().and_then(controls::external_url).is_none();
                let menu = menu
                    .action_context(focus.clone())
                    .menu("Reload", Box::new(BrowserReload))
                    .menu("Duplicate tab", Box::new(BrowserDuplicate))
                    .menu_with_disabled("Copy URL", Box::new(BrowserCopyUrl), no_page)
                    .menu_with_disabled(
                        "Open in system browser",
                        Box::new(BrowserOpenExternal),
                        no_web,
                    )
                    .separator()
                    .menu("Find in page", Box::new(BrowserFind))
                    .menu("Zoom in", Box::new(BrowserZoomIn))
                    .menu("Zoom out", Box::new(BrowserZoomOut))
                    .menu("Actual size", Box::new(BrowserZoomReset));
                #[cfg(debug_assertions)]
                let menu = menu
                    .separator()
                    .menu("Open DevTools", Box::new(BrowserDevTools));
                menu
            })
            .on_open_change(move |open, _, cx| {
                let _ = reopen.update(cx, |b, cx| b.set_menu_open(*open, cx));
            })
            .into_any_element()
    }

    /// The find bar, as a row under the toolbar.
    pub(super) fn render_find(&self, _: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let find = self.find.as_ref()?;
        let theme = cx.theme();
        let step = |id: &'static str, icon: IconName, backwards: bool| {
            Button::new(id)
                .icon(icon)
                .xsmall()
                .ghost()
                .disabled(find.missed)
                .on_click(cx.listener(move |this, _, _, cx| this.run_find(backwards, false, cx)))
        };
        Some(
            h_flex()
                .key_context(controls::FIND_CONTEXT)
                .gap_1()
                .px_1()
                .py_1()
                .border_b_1()
                .border_color(theme.border)
                .child(div().w(px(260.)).child(Input::new(&find.input).small()))
                .when(find.missed, |row| {
                    row.child(div().text_xs().text_color(theme.danger).child("No matches"))
                })
                .child(step("find-prev", IconName::ChevronUp, true))
                .child(step("find-next", IconName::ChevronDown, false))
                .child(
                    Button::new("find-close")
                        .icon(IconName::Close)
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_find(&BrowserCloseFind, window, cx)
                        })),
                )
                .into_any_element(),
        )
    }
}
