//! `WKWebView takeSnapshotWithConfiguration:completionHandler:`, which objc2 binds only as an `unsafe fn`, and the
//! private `_inspector detach` that puts DevTools in a window of its own. Everything else (decoding, scaling, size caps) stays in safe code in skyelab-browser. macOS only.
#![allow(unsafe_code)]

#[cfg(target_os = "macos")]
pub use mac::{detach_inspector, snapshot};

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;

    use block2::RcBlock;
    use objc2::{rc::Retained, runtime::AnyObject, sel};
    use objc2_app_kit::NSImage;
    use objc2_foundation::NSError;
    use objc2_web_kit::WKWebView;

    /// Snapshots what `view` shows now, at the screen's scale, and calls `done` once on the main thread with the
    /// image as TIFF bytes, or WebKit's error. No configuration, so the rect is the view's own bounds: nothing
    /// outside the web view is ever captured.
    pub fn snapshot(view: &WKWebView, done: impl FnOnce(Result<Vec<u8>, String>) + 'static) {
        // WebKit wants an `Fn` block; it calls it once.
        let done = RefCell::new(Some(done));
        let block = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
            let Some(done) = done.borrow_mut().take() else {
                return;
            };
            // SAFETY: WebKit passes either null or a valid NSImage/NSError that lives for the block's call.
            let (image, error) = unsafe { (image.as_ref(), error.as_ref()) };
            done(match (image, error) {
                (Some(image), _) => image
                    .TIFFRepresentation()
                    .map(|data| data.to_vec())
                    .ok_or_else(|| "the snapshot has no bitmap".to_owned()),
                (None, Some(error)) => Err(error.localizedDescription().to_string()),
                (None, None) => Err("WebKit returned no snapshot".to_owned()),
            });
        });
        // SAFETY: `view` is a live WKWebView borrowed on the main thread (WKWebView is MainThreadOnly, so the
        // reference proves it). A nil configuration is documented as "the view's bounds". WebKit copies the block,
        // so dropping our `RcBlock` after the call is fine.
        unsafe { view.takeSnapshotWithConfiguration_completionHandler(None, &block) };
    }

    /// Moves `view`'s open Web Inspector into a window of its own. Docked, WebKit lays it out over the whole host
    /// window and stretches the page across it, covering the GPUI views beside the pane. Does nothing when this
    /// WebKit has no such private method.
    pub fn detach_inspector(view: &WKWebView) {
        // SAFETY: `_inspector` is WKWebView's private getter for its `_WKInspector` (what wry's `open_devtools`
        // calls), checked with respondsToSelector first; it returns a retained object or nil, never anything else.
        let inspector: Option<Retained<AnyObject>> = unsafe {
            if !objc2::msg_send![view, respondsToSelector: sel!(_inspector)] {
                return;
            }
            objc2::msg_send![view, _inspector]
        };
        let Some(inspector) = inspector else { return };
        // SAFETY: `detach` is `_WKInspector`'s `- (void)detach`, checked first; called on the main thread (the
        // `&WKWebView`, which is MainThreadOnly, proves it), as WebKit requires.
        unsafe {
            if objc2::msg_send![&inspector, respondsToSelector: sel!(detach)] {
                let () = objc2::msg_send![&inspector, detach];
            }
        }
    }
}
