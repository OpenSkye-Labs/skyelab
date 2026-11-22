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
//!   away. The workspace does this from the dock's `set_active`, which arrives a tick late, so the
//!   page can sit over the next tab for one frame.
//! - Two paths in: the user's URL bar ([`parse_url`]: http, https, local file) and the agent API
//!   (`navigate`, `current_url`, `eval_js`; [`parse_agent_url`]: http and https only). `eval_js`
//!   refuses before the first load and on any page that isn't http or https, since a script in a
//!   `file://` page can read local files. Only that input is checked. wry's
//!   navigation handler also fires for iframes, so filtering there would break `about:`, `data:`
//!   and `blob:` frames that real web apps use; links and scripts in the page go unfiltered.
//! - No screenshot API: wry has none. Capture the window instead.

mod agent;
mod js;
mod url;
mod view;

pub use agent::parse_agent_url;
pub use url::parse_url;
pub use view::BrowserView;
