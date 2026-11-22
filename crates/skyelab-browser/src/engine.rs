//! What the agent path needs from a page engine. wry is the real one; tests plug in a fake with
//! [`crate::BrowserView::with_engine`], so they run the same URL checks, page guard and decoding.

use std::sync::Mutex;

use anyhow::Context as _;
use gpui::{App, Entity};
use gpui_webview::WebView;

/// The script's raw result, as wry hands it back (a JSON-encoded string).
pub type EvalDone = Box<dyn FnOnce(String) + Send>;

pub trait Engine {
    fn load(&self, url: &str, cx: &App) -> anyhow::Result<()>;
    /// The URL as the engine reports it, which may name a pending load.
    fn url(&self, cx: &App) -> anyhow::Result<String>;
    /// Runs `js` and calls `done` with its result. An engine may drop `done` (wry does for a script
    /// it queued for the next load).
    fn eval(&self, js: &str, done: EvalDone, cx: &App) -> anyhow::Result<()>;
}

pub(crate) struct Wry(pub Entity<WebView>);

impl Engine for Wry {
    fn load(&self, url: &str, cx: &App) -> anyhow::Result<()> {
        Ok(self.0.read(cx).raw().load_url(url)?)
    }

    fn url(&self, cx: &App) -> anyhow::Result<String> {
        Ok(self.0.read(cx).url()?)
    }

    fn eval(&self, js: &str, done: EvalDone, cx: &App) -> anyhow::Result<()> {
        // wry wants `Fn`; the result comes once.
        let done = Mutex::new(Some(done));
        self.0
            .read(cx)
            .evaluate_script_with_callback(js, move |raw| {
                if let Some(done) = done.lock().ok().and_then(|mut d| d.take()) {
                    done(raw);
                }
            })
            .context("evaluate script")
    }
}
