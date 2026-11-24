//! A WebView2 page snapshot: wry has no API for it, and `CapturePreview` plus the memory stream it writes into are
//! `unsafe` COM calls. Windows only; macOS has `skyelab-webkit-sys`, Linux WebKitGTK's safe snapshot.
#![allow(unsafe_code)]

#[cfg(windows)]
pub use win::snapshot;

#[cfg(windows)]
mod win {
    use std::cell::RefCell;
    use std::rc::Rc;

    use webview2_com::CapturePreviewCompletedHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, ICoreWebView2,
    };
    use windows::Win32::System::Com::{IStream, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET};
    use windows::Win32::UI::Shell::SHCreateMemStream;

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
