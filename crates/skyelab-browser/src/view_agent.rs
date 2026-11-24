//! What an agent (through SkyeLab's MCP tools) reads and runs in a tab: its URL, title, scripts, screenshots.

use super::*;

impl BrowserView {
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

    /// The page's title after its last load; `None` while it has none.
    pub fn title(&self) -> Option<&SharedString> {
        self.title.as_ref()
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
}
