//! The WebView2 calls wry has no API for, all `unsafe` COM: a page snapshot (`CapturePreview` plus the memory
//! stream it writes into), and the network lock of a visual reply's page. Windows only; macOS has
//! `skyelab-webkit-sys`, Linux WebKitGTK's safe snapshot.
#![allow(unsafe_code)]

#[cfg(windows)]
pub use win::{lock, snapshot};

/// Whether the lock lets a request through: what has no network (`data:`, `blob:`, `about:`), and a script, style
/// sheet or font from `https://` one of `hosts` (the page CSP's list, `skyelab_browser::page::csp`; `*.name` is
/// any subdomain of `name`). No image, media or other kind ever reaches the network. `resource` is the request's kind.
pub fn lock_allows(uri: &str, resource: Resource, hosts: &[String]) -> bool {
    let scheme = uri.split_once(':').map(|(s, _)| s.to_ascii_lowercase());
    match scheme.as_deref() {
        Some("data" | "blob" | "about") => true,
        Some("https") => {
            matches!(
                resource,
                Resource::Script | Resource::Stylesheet | Resource::Font
            ) && https_host(uri).is_some_and(|host| {
                hosts.iter().any(|h| match h.strip_prefix("*.") {
                    Some(base) => host
                        .strip_suffix(base)
                        .is_some_and(|sub| sub.len() > 1 && sub.ends_with('.')),
                    None => &host == h,
                })
            })
        }
        _ => false,
    }
}

/// The lowercase host of an `https://` URI on the default port, `None` for anything unusual (credentials, another
/// port, an IPv6 literal): a CSP `https://host` source matches none of those either.
fn https_host(uri: &str) -> Option<String> {
    let rest = uri
        .get(..8)
        .filter(|s| s.eq_ignore_ascii_case("https://"))
        .map(|_| &uri[8..])?;
    let authority = rest.split(['/', '?', '#', '\\']).next()?;
    let host = authority.strip_suffix(":443").unwrap_or(authority);
    (!host.is_empty() && !host.contains(['@', ':', '[', ']', '%']))
        .then(|| host.to_ascii_lowercase())
}

/// The kinds of request the lock tells apart; every other kind is `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Script,
    Stylesheet,
    Image,
    Font,
    Media,
    Other,
}

#[cfg(test)]
mod tests {
    use super::{Resource, lock_allows};

    #[test]
    fn lock_passes_only_what_the_csp_allows() {
        let hosts = ["cdn.example".to_owned(), "*.fonts.example".to_owned()];
        let allows = |uri, resource| lock_allows(uri, resource, &hosts);
        assert!(allows("data:text/html,hi", Resource::Other));
        assert!(allows("about:blank", Resource::Other));
        assert!(allows("blob:null/1", Resource::Image));
        assert!(allows("https://cdn.example/a.js", Resource::Script));
        assert!(allows(
            "HTTPS://CDN.example:443/a.css",
            Resource::Stylesheet
        ));
        assert!(allows("https://a.b.fonts.example/f.woff2", Resource::Font));
        // Images and media load from data: and blob: only, even from an allowed host.
        assert!(!allows("https://cdn.example/a.png", Resource::Image));
        assert!(!allows("https://cdn.example/a.mp4", Resource::Media));
        assert!(!allows("https://cdn.example/", Resource::Other));
        // Hosts not on the list, and URLs that only look like them.
        assert!(!allows("https://evil.example/a.js", Resource::Script));
        assert!(!allows(
            "https://cdn.example.evil.example/a.js",
            Resource::Script
        ));
        assert!(!allows("https://xcdn.example/a.js", Resource::Script));
        assert!(!allows("https://fonts.example/f.woff2", Resource::Font));
        assert!(!allows("https://evilfonts.example/f.woff2", Resource::Font));
        assert!(!allows(
            "https://cdn.example@evil.example/a.js",
            Resource::Script
        ));
        assert!(!allows("https://cdn.example:8443/a.js", Resource::Script));
        assert!(!allows("https://[::1]/a.js", Resource::Script));
        assert!(!allows("http://cdn.example/a.js", Resource::Script));
        assert!(!allows("http://127.0.0.1/img", Resource::Image));
        assert!(!allows("ws://127.0.0.1/ws", Resource::Other));
        assert!(!allows("file:///c:/x", Resource::Script));
        assert!(!allows("no-scheme", Resource::Image));
        assert!(!lock_allows(
            "https://cdn.example/a.js",
            Resource::Script,
            &[]
        ));
    }
}

#[cfg(windows)]
mod win {
    use std::cell::RefCell;
    use std::rc::Rc;

    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, COREWEBVIEW2_WEB_RESOURCE_CONTEXT,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET,
        COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL, ICoreWebView2, ICoreWebView2_2,
        ICoreWebView2_22,
    };
    use webview2_com::{
        CapturePreviewCompletedHandler, NewWindowRequestedEventHandler,
        WebResourceRequestedEventHandler, take_pwstr,
    };
    use windows::Win32::System::Com::{IStream, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET};
    use windows::Win32::UI::Shell::SHCreateMemStream;
    use windows::core::{Interface as _, PWSTR, w};

    use crate::{Resource, lock_allows};

    /// Locks a visual reply's page to what its CSP allows, under the CSP rather than instead of it: every request
    /// [`lock_allows`] refuses is answered here with an empty 403, so it never reaches the network, and every new
    /// window is refused on the spot. Call it on the UI thread before the page loads, so its first request is
    /// already covered. WebView2 can't filter WebSockets; the CSP's `connect-src 'none'` stops those.
    pub fn lock(webview: &ICoreWebView2, hosts: Vec<String>) -> windows::core::Result<()> {
        let env = webview.cast::<ICoreWebView2_2>()?;
        // SAFETY: `webview` is a live ICoreWebView2; Environment returns a new reference we own.
        let env = unsafe { env.Environment()? };
        let requested = WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut uri = PWSTR::null();
            let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
            // SAFETY: `args` is the live event args WebView2 handed us on this thread; Uri hands back a CoTaskMem
            // string that take_pwstr frees, ResourceContext fills `context`.
            let (uri, context) = unsafe {
                args.Request()?.Uri(&mut uri)?;
                args.ResourceContext(&mut context)?;
                (take_pwstr(uri), context)
            };
            if lock_allows(&uri, resource(context), &hosts) {
                return Ok(());
            }
            // SAFETY: `env` is the webview's live environment; no content stream and static strings. SetResponse
            // takes its own reference to the response.
            unsafe {
                let blocked = env.CreateWebResourceResponse(None, 403, w!("Blocked"), w!(""))?;
                args.SetResponse(&blocked)
            }
        }));
        // Handled with no new window: the request is dropped. Set here, on the UI thread, because wry answers
        // from a thread of its own, where WebView2's objects refuse the call.
        let new_window = NewWindowRequestedEventHandler::create(Box::new(|_, args| match args {
            // SAFETY: `args` is the live event args WebView2 handed us on this thread.
            Some(args) => unsafe { args.SetHandled(true) },
            None => Ok(()),
        }));
        let mut token = 0i64;
        // SAFETY: `webview` and the handlers are live COM objects; WebView2 AddRefs a handler for as long as it is
        // registered and calls it on this (the UI) thread. `token` outlives each call.
        unsafe {
            match webview.cast::<ICoreWebView2_22>() {
                // Requests from workers too (the CSP forbids workers; this is belt and braces).
                Ok(webview22) => webview22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                    w!("*"),
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                )?,
                Err(_) => webview.AddWebResourceRequestedFilter(
                    w!("*"),
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                )?,
            }
            webview.add_WebResourceRequested(&requested, &mut token)?;
            webview.add_NewWindowRequested(&new_window, &mut token)?;
        }
        Ok(())
    }

    fn resource(context: COREWEBVIEW2_WEB_RESOURCE_CONTEXT) -> Resource {
        match context {
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => Resource::Script,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => Resource::Stylesheet,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => Resource::Image,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => Resource::Font,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => Resource::Media,
            _ => Resource::Other,
        }
    }

    type Done = Box<dyn FnOnce(Result<Vec<u8>, String>)>;

    /// Captures what `webview` shows now (its own area only) as a PNG and calls `done` once with it, on the UI
    /// thread, when WebView2 finishes.
    pub fn snapshot(webview: &ICoreWebView2, done: impl FnOnce(Result<Vec<u8>, String>) + 'static) {
        // SAFETY: with no initial data SHCreateMemStream reads nothing and returns a new, empty stream we own.
        let Some(stream) = (unsafe { SHCreateMemStream(None) }) else {
            return done(Err("couldn't make a memory stream".to_owned()));
        };
        // Shared: the handler takes it when the capture ends, this function when CapturePreview fails at once.
        let done: Rc<RefCell<Option<Done>>> = Rc::new(RefCell::new(Some(Box::new(done))));
        let (in_handler, read_back) = (done.clone(), stream.clone());
        let handler = CapturePreviewCompletedHandler::create(Box::new(move |result| {
            if let Some(done) = in_handler.borrow_mut().take() {
                done(
                    result
                        .and_then(|()| read_all(&read_back))
                        .map_err(|e| format!("WebView2 capture: {e}")),
                );
            }
            Ok(())
        }));
        // SAFETY: `webview`, `stream` and `handler` are live COM objects; CapturePreview AddRefs the stream and the
        // handler for as long as the capture runs, and calls the handler on this (the UI) thread.
        let started = unsafe {
            webview.CapturePreview(
                COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                &stream,
                &handler,
            )
        };
        if let Err(e) = started
            && let Some(done) = done.borrow_mut().take()
        {
            done(Err(format!("WebView2 capture: {e}")));
        }
    }

    /// The whole stream, from its start.
    fn read_all(stream: &IStream) -> windows::core::Result<Vec<u8>> {
        let mut stat = STATSTG::default();
        // SAFETY: `stat` is a live STATSTG for Stat to fill; NONAME means it allocates no name for us to free.
        unsafe { stream.Stat(&mut stat, STATFLAG_NONAME)? };
        // A PNG of one webview's area: far below 4 GiB, which a u32 read can't pass anyway.
        let len = u32::try_from(stat.cbSize).map_err(|_| windows::core::Error::empty())?;
        let mut out = vec![0u8; len as usize];
        let mut filled = 0usize;
        // SAFETY: Seek takes no pointers here.
        unsafe { stream.Seek(0, STREAM_SEEK_SET, None)? };
        // IStream may return fewer bytes than asked; read until the stream ends or `out` is full.
        while filled < out.len() {
            let mut read = 0u32;
            // SAFETY: `out[filled..]` has `len - filled` writable bytes; `read` is a live u32 for Read.
            unsafe {
                stream
                    .Read(
                        out[filled..].as_mut_ptr().cast(),
                        len - filled as u32,
                        Some(&mut read),
                    )
                    .ok()?;
            }
            if read == 0 {
                break;
            }
            filled += read as usize;
        }
        out.truncate(filled);
        Ok(out)
    }
}
