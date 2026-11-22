//! Embedded browser preview: a URL bar and a native webview, so users and agents can look at the
//! web app being built.
//!
//! Built on `gpui-webview` (wry), which is **experimental**. Known limits:
//! - Engines: WKWebView on macOS, WebView2 on Windows, WebKitGTK on Linux.
//! - Linux needs X11 or XWayland. Wayland windows are not supported (`WebView::build` errors).
//! - The page is a native view on top of the GPUI window. It draws over GPUI overlays (menus,
//!   popovers, dialogs, tooltips, the dock's drag-and-drop targets) in its bounds, so don't put
//!   overlays over the page. (`gpui-webview`'s `gpui-fast` feature fixes this but means a second
//!   GPUI, which we don't allow.)
//! - The native view also stays on screen when its pane isn't drawn. Whoever hosts the pane calls
//!   [`BrowserView::set_visible`] when it is shown or hidden and [`BrowserView::close`] when it goes
//!   away.
//! - Two paths in: the user's URL bar ([`parse_url`]: http, https, local file) and the agent API
//!   (`navigate`, `current_url`, `eval_js`; [`parse_agent_url`]: http and https only). `eval_js`
//!   refuses before the first load and in any page that isn't http or https, since a script in a
//!   `file://` page can read local files. The script itself checks `location` first, because the
//!   engine's URL names a pending load while the old page still runs scripts. This limits the
//!   preview, not the agent: an agent process can still read files directly or serve them over
//!   `http://localhost`. Only that input is checked. wry's
//!   navigation handler also fires for iframes, so filtering there would break `about:`, `data:`
//!   and `blob:` frames that real web apps use; links and scripts in the page go unfiltered.
//! - No screenshot API: wry has none, and WKWebView's snapshot is an `unsafe` call. Capture the window instead.
//! - [`Engine`] is the seam under the agent path: [`BrowserView::with_engine`] runs it on a test's fake page.

mod agent;
mod engine;
mod js;
mod url;
mod view;

pub use agent::parse_agent_url;
pub use engine::{Engine, EvalDone};
pub use url::parse_url;
pub use view::BrowserView;
