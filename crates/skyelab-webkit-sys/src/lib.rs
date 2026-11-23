//! `WKWebView takeSnapshotWithConfiguration:completionHandler:`, which objc2 binds only as an `unsafe fn`.
//! Everything else (decoding, scaling, size caps) stays in safe code in skyelab-browser. macOS only.
#![allow(unsafe_code)]

#[cfg(target_os = "macos")]
pub use mac::snapshot;

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;

    use block2::RcBlock;
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
}
