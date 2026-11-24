//! An agent's visual reply (`html_render`): an untrusted page shown in a locked, private, throwaway webview.
//!
//! - The page loads from a string (`loadHTMLString` with no base URL): an opaque `about:blank` origin with no file
//!   access. Every later navigation, new window and download is refused, there is no IPC handler and no custom
//!   scheme, and each page gets its own in-memory web data store (on Windows a fresh temp folder).
//! - [`prepare`] puts a doctype and a Content-Security-Policy meta first in the document, so it governs everything after it:
//!   scripts, styles, images, fonts and media from inline code, `data:`/`blob:` or https only; no fetch, XHR,
//!   WebSocket, frames, workers, plugins, forms or `<base>`.
//! - Then T3's theme variables and base sheet, which the page may override.
//! - [`shoot`] lays a page out hidden at a width, measures it and snapshots it; the timeline shows that image, so
//!   no native view ever sits inside a scrolling list. The live page is only for the expand dialog.

use std::time::Duration;

use anyhow::{Context as _, anyhow};
use futures::{FutureExt as _, channel::oneshot};
use gpui::{App, AppContext as _, Entity, Task, Window};
use gpui_webview::WebView;

use crate::{Engine as _, engine::Wry};

/// The policy every page runs under. A meta CSP can't loosen one that is already there, and nothing in the page
/// comes before it.
pub const CSP: &str = "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' https:; \
    style-src 'unsafe-inline' https:; img-src data: blob: https:; font-src data: https:; \
    media-src data: blob: https:; connect-src 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'; \
    object-src 'none'; manifest-src 'none'; form-action 'none'; base-uri 'none'";

/// T3's base sheet: an 8px gutter, theme background, text and font, focus rings inside the element.
const BASE_CSS: &str = "html{box-sizing:border-box;padding:8px;background:var(--background);\
    color:var(--foreground);font-family:var(--font-sans);font-size:14px;line-height:1.5;\
    -webkit-font-smoothing:antialiased;-webkit-text-size-adjust:100%}body{margin:0}\
    code,kbd,pre,samp{font-family:var(--font-mono)}:where(:focus-visible){outline:2px solid var(--ring)}";

/// The tallest snapshot, in CSS pixels (T3's largest frame); the expand dialog shows the rest.
pub const MAX_SHOT_HEIGHT: u32 = 2000;
/// Smallest frame, and the height a page is first laid out at, so a short page measures as its content.
pub const MIN_SHOT_HEIGHT: u32 = 80;
/// A shot gives up after this.
pub const SHOT_TIMEOUT: Duration = Duration::from_secs(10);
/// After the load, before measuring: lets scripts draw and a chart's entry animation mostly finish.
const SETTLE: Duration = Duration::from_millis(600);

/// The theme a page gets: `--name` → CSS value pairs, T3's names (`--background`, `--chart-1`, ...).
#[derive(Debug, Clone, PartialEq)]
pub struct PageTheme {
    pub dark: bool,
    pub vars: Vec<(&'static str, String)>,
}

/// The document the webview loads: `html` with the CSP and the theme put in front of it.
pub fn prepare(html: &str, theme: &PageTheme) -> String {
    let mut root = format!(
        ":root{{color-scheme:{};",
        if theme.dark { "dark" } else { "light" }
    );
    for (name, value) in &theme.vars {
        // Our own values, but a font name comes from settings: nothing that ends the rule or the tag.
        let value: String = value
            .chars()
            .filter(|c| !matches!(c, ';' | '{' | '}' | '<' | '>'))
            .collect();
        root.push_str(&format!("{name}:{value};"));
    }
    root.push('}');
    let head = format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">\
         <style id=\"skyelab-theme\">{root}{BASE_CSS}</style>"
    );
    // Ours always comes first: anything of the page's before the meta (even a character the HTML parser doesn't
    // count as space, like U+000B) would open <body>, and a CSP meta outside <head> is ignored. The page's own
    // doctype, after ours, is ignored too.
    format!("<!doctype html>{head}{html}")
}

/// A page in its own locked webview, hidden until the host shows it.
pub struct Page {
    webview: Entity<WebView>,
    loaded: flume::Receiver<()>,
    _data_dir: Option<tempfile::TempDir>,
}

impl Page {
    /// Builds the webview on `html` (already [`prepare`]d) in `window`, hidden.
    pub fn new(prepared: String, window: &mut Window, cx: &mut App) -> anyhow::Result<Self> {
        let override_env = std::env::var_os("WEBVIEW2_USER_DATA_FOLDER");
        let data_dir = crate::webview_data_dir(cfg!(windows), true, override_env.as_deref())?;
        let mut context = data_dir
            .as_ref()
            .map(|d| wry::WebContext::new(Some(d.path().to_owned())));
        let builder = match context.as_mut() {
            Some(context) => wry::WebViewBuilder::new_with_web_context(context),
            None => wry::WebViewBuilder::new(),
        };
        let (loaded_tx, loaded) = flume::unbounded();
        let builder = builder
            .with_incognito(true)
            .with_devtools(false)
            .with_visible(false)
            .with_html(prepared)
            // The string load is the one navigation; a link, a script or a meta refresh goes nowhere.
            .with_navigation_handler(|url| url == "about:blank")
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_download_started_handler(|_, _| false)
            .with_on_page_load_handler(move |event, _| {
                if matches!(event, wry::PageLoadEvent::Finished) {
                    let _ = loaded_tx.send(());
                }
            });
        let webview = WebView::build(builder, window, cx).context("create the page's webview")?;
        let _ = webview.set_visible(false);
        Ok(Self {
            webview: cx.new(|_| webview),
            loaded,
            _data_dir: data_dir,
        })
    }

    /// The webview, to draw (the expand dialog) or show and hide.
    pub fn webview(&self) -> &Entity<WebView> {
        &self.webview
    }
}

/// A page's snapshot: the engine's image (TIFF on macOS, PNG on Linux) and the page's full height in CSS pixels.
pub struct PageShot {
    pub image: Vec<u8>,
    pub content_height: u32,
}

impl PageShot {
    /// The image as a PNG at full size (the card draws it at the screen's scale). Slow: off the UI thread.
    pub fn to_png(&self) -> anyhow::Result<Vec<u8>> {
        let image = image::load_from_memory(&self.image).context("decode the snapshot")?;
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(image.to_rgba8())
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .context("encode the PNG")?;
        Ok(png)
    }
}

/// Lays `html` out hidden at `width` CSS pixels and snapshots it, at most [`MAX_SHOT_HEIGHT`] tall. The webview is
/// dropped when the task ends. Windows has no snapshot yet: an error.
pub fn shoot(
    html: &str,
    theme: &PageTheme,
    width: u32,
    window: &mut Window,
    cx: &mut App,
) -> Task<anyhow::Result<PageShot>> {
    let page = match Page::new(prepare(html, theme), window, cx) {
        Ok(page) => page,
        Err(e) => return Task::ready(Err(e)),
    };
    let executor = cx.background_executor().clone();
    let timeout = executor.timer(SHOT_TIMEOUT);
    cx.spawn(async move |cx| {
        let work = async {
            set_size(&page, width, MIN_SHOT_HEIGHT, cx)?;
            page.loaded
                .recv_async()
                .await
                .map_err(|_| anyhow!("the page never finished loading"))?;
            executor.timer(SETTLE).await;
            let content_height = measure(&page, cx).await?;
            set_size(
                &page,
                width,
                content_height.clamp(MIN_SHOT_HEIGHT, MAX_SHOT_HEIGHT),
                cx,
            )?;
            executor.timer(Duration::from_millis(200)).await;
            let (tx, rx) = oneshot::channel();
            cx.update(|cx| {
                Wry(page.webview.clone()).snapshot(Box::new(move |r| _ = tx.send(r)), cx)
            })?;
            let image = rx
                .await
                .context("the webview dropped the snapshot")?
                .map_err(|e| anyhow!("snapshot failed: {e}"))?;
            anyhow::Ok(PageShot {
                image,
                content_height,
            })
        };
        futures::select! {
            r = work.fuse() => r,
            () = timeout.fuse() => Err(anyhow!("the page did not render in {}s", SHOT_TIMEOUT.as_secs())),
        }
    })
}

fn set_size(page: &Page, width: u32, height: u32, cx: &mut gpui::AsyncApp) -> anyhow::Result<()> {
    let rect = wry::Rect {
        position: wry::dpi::LogicalPosition::new(0.0, 0.0).into(),
        size: wry::dpi::LogicalSize::new(f64::from(width), f64::from(height)).into(),
    };
    cx.read_entity(&page.webview, |w, _| w.set_bounds(rect))
        .context("size the page")
}

/// The page's height. The script returns a number it made itself: wry's macOS bridge aborts the app on a result
/// it can't encode (NaN), and the page owns every getter it reads.
async fn measure(page: &Page, cx: &mut gpui::AsyncApp) -> anyhow::Result<u32> {
    const SCRIPT: &str = "(function(){try{var h=document.documentElement.scrollHeight|0;return h>0?h:0}\
        catch(e){return 0}})()";
    let (tx, rx) = oneshot::channel();
    let tx = std::sync::Mutex::new(Some(tx));
    cx.read_entity(&page.webview, |w, _| {
        w.raw().evaluate_script_with_callback(SCRIPT, move |raw| {
            if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
                let _ = tx.send(raw);
            }
        })
    })
    .context("measure the page")?;
    let raw = rx.await.context("the webview dropped the measurement")?;
    Ok(raw.trim().parse::<u32>().unwrap_or(0).max(MIN_SHOT_HEIGHT))
}

#[cfg(test)]
mod tests;
