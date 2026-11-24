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
//! - Screenshots (`screenshot`, the agent path's rules): macOS through WKWebView's snapshot in `skyelab-webkit-sys`
//!   (wry has no API and the call is `unsafe`), Linux through WebKitGTK's safe `WebView::snapshot` (visible region,
//!   encoded by cairo; cross-checked only, never run), Windows through WebView2's `CapturePreview` (PNG) in
//!   `skyelab-webview2-sys` (cross-compiled here; Windows CI runs it in `wkwebview_page`).
//! - A failed load shows GPUI's "This site can't be reached" ([`load_error`]) in place of the hidden page. Only
//!   WKWebView needs it (WebKitGTK and WebView2 draw their own error page); its failures come from the delegate proxy
//!   in `skyelab-webkit-sys`, since wry reports none.
//! - [`Engine`] is the seam under the agent path: [`BrowserView::with_engine`] runs it on a test's fake page.

mod agent;
mod controls;
mod engine;
mod js;
pub mod load_error;
pub mod page;
mod shot;
mod url;
mod view;

/// The user-data folder a webview gets: none for the user's tabs and off Windows (macOS and Linux give each private
/// webview its own in-memory store), a fresh private one for each agent tab on Windows, where WebView2 shares one
/// InPrivate session between the webviews of a folder. `override_env` is `WEBVIEW2_USER_DATA_FOLDER`, which can
/// make WebView2 ignore the folder: then an agent tab is refused rather than share a session.
pub fn webview_data_dir(
    windows: bool,
    private: bool,
    override_env: Option<&std::ffi::OsStr>,
) -> anyhow::Result<Option<tempfile::TempDir>> {
    if !(windows && private) {
        return Ok(None);
    }
    if override_env.is_some_and(|v| !v.is_empty()) {
        anyhow::bail!(
            "WEBVIEW2_USER_DATA_FOLDER is set, so agent tabs can't get a web data folder of their own; unset it"
        );
    }
    Ok(Some(agent_data_dir()?))
}

/// A fresh private folder for one agent tab's web data, removed when dropped.
pub fn agent_data_dir() -> std::io::Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("skyelab-agent-tab-");
    #[cfg(unix)]
    builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700));
    builder.tempdir()
}

pub use agent::parse_agent_url;
pub use controls::{
    BrowserCloseFind, BrowserCopyUrl, BrowserDevTools, BrowserDuplicate, BrowserFind,
    BrowserOpenExternal, BrowserReload, BrowserZoomIn, BrowserZoomOut, BrowserZoomReset, FIND_KEY,
    ZOOM_IN_KEY, ZOOM_OUT_KEY, ZOOM_RESET_KEY, bind_keys,
};
pub use engine::{Engine, EvalDone, FindDone, SnapshotDone};
pub use js::MAX_EVAL_OUTPUT;
pub use shot::{MAX_SHOT_BYTES, MAX_SHOT_WIDTH, Shot, encode_png};
pub use url::parse_url;
pub use view::{BrowserView, DuplicateTab};
