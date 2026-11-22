//! Embedded browser preview: a URL bar and a native webview, so users and agents can look at the
//! web app being built.
//!
//! Built on `gpui-webview` (wry), which is **experimental**. Known limits:
//! - Engines: WKWebView on macOS, WebView2 on Windows, WebKitGTK on Linux.
//! - Linux needs X11 or XWayland. Wayland windows are not supported (`WebView::build` errors).
//! - The page is a native view on top of the GPUI window. It draws over GPUI overlays (menus,
//!   popovers, dialogs, tooltips) in its bounds, so don't put overlays over the page.
//!   (`gpui-webview`'s `gpui-fast` feature fixes this but means a second GPUI, which we don't allow.)
//! - Only user and agent input is checked by [`parse_url`] (http, https, local file). wry's
//!   navigation handler also fires for iframes, so filtering there would break `about:`, `data:`
//!   and `blob:` frames that real web apps use; links and scripts in the page go unfiltered.
//! - No screenshot API: wry has none. Capture the window instead.

mod js;
mod url;
mod view;

pub use url::parse_url;
pub use view::BrowserView;
