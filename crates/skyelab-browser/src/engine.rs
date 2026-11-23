//! What the agent path needs from a page engine. wry is the real one; tests plug in a fake with
//! [`crate::BrowserView::with_engine`], so they run the same URL checks, page guard and decoding.

use std::sync::Mutex;

use anyhow::Context as _;
use gpui::{App, Entity};
use gpui_webview::WebView;

/// The script's raw result, as wry hands it back (a JSON-encoded string).
pub type EvalDone = Box<dyn FnOnce(String) + Send>;

/// A snapshot's encoded image (TIFF from WebKit, any format `image` reads), or the engine's error.
pub type SnapshotDone = Box<dyn FnOnce(Result<Vec<u8>, String>)>;

pub trait Engine {
    fn load(&self, url: &str, cx: &App) -> anyhow::Result<()>;
    /// The URL as the engine reports it, which may name a pending load.
    fn url(&self, cx: &App) -> anyhow::Result<String>;
    /// Runs `js` (from `js::wrap`) and calls `done` with its answer, JSON-encoded. An engine may drop `done`
    /// (wry does for a script it queued for the next load).
    fn eval(&self, js: &str, done: EvalDone, cx: &App) -> anyhow::Result<()>;
    /// Captures what the page shows now (only the web view's own area) and calls `done` once with it.
    fn snapshot(&self, done: SnapshotDone, cx: &App) -> anyhow::Result<()> {
        let _ = (done, cx);
        anyhow::bail!("screenshots are not supported on this platform yet")
    }
    /// Loads the current page again.
    fn reload(&self, cx: &App) -> anyhow::Result<()> {
        self.load(&self.url(cx)?, cx)
    }
}

pub(crate) struct Wry(pub Entity<WebView>);

impl Engine for Wry {
    fn load(&self, url: &str, cx: &App) -> anyhow::Result<()> {
        Ok(self.0.read(cx).raw().load_url(url)?)
    }

    fn url(&self, cx: &App) -> anyhow::Result<String> {
        Ok(self.0.read(cx).url()?)
    }

    fn reload(&self, cx: &App) -> anyhow::Result<()> {
        Ok(self.0.read(cx).reload()?)
    }

    #[cfg(target_os = "macos")]
    fn snapshot(&self, done: SnapshotDone, cx: &App) -> anyhow::Result<()> {
        use wry::WebViewExtMacOS as _;
        skyelab_webkit_sys::snapshot(&self.0.read(cx).raw().webview(), done);
        Ok(())
    }

    fn eval(&self, js: &str, done: EvalDone, cx: &App) -> anyhow::Result<()> {
        let page = self.0.read(cx);
        // No callback, so wry never serializes what the agent's script evaluates to.
        page.evaluate_script(js).context("evaluate script")?;
        // wry wants `Fn`; the result comes once.
        let done = Mutex::new(Some(done));
        page.evaluate_script_with_callback(&crate::js::read_back(), move |raw| {
            if let Some(done) = done.lock().ok().and_then(|mut d| d.take()) {
                done(raw);
            }
        })
        .context("evaluate script")
    }
}
