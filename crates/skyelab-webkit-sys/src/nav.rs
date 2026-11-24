//! A page load that fails: wry reports only Started and Finished, so a refused connection leaves WKWebView blank
//! with nothing said. This wraps wry's navigation delegate in a proxy that also answers WebKit's two failure calls
//! and forwards every other message to wry's delegate unchanged.

use objc2::{
    DefinedClass as _, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::{Retained, Weak},
    runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel},
};
use objc2_foundation::{NSError, NSString, NSURL};
use objc2_web_kit::{WKNavigationDelegate, WKWebView};

/// What WebKit said about a failed load of the main frame.
#[derive(Debug, Clone)]
pub struct LoadFailure {
    /// `NSURLErrorDomain`, `WebKitErrorDomain`, …
    pub domain: String,
    pub code: isize,
    pub description: String,
    /// The URL that failed (`NSErrorFailingURLKey`), when WebKit names it.
    pub url: Option<String>,
}

struct Ivars {
    inner: Retained<ProtocolObject<dyn WKNavigationDelegate>>,
    on_fail: Box<dyn Fn(LoadFailure)>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SkyeLabNavigationFailureProxy"]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Proxy;

    unsafe impl NSObjectProtocol for Proxy {}

    unsafe impl WKNavigationDelegate for Proxy {
        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail_provisional(&self, _view: &WKWebView, _nav: Option<&AnyObject>, error: &NSError) {
            self.failed(error);
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(&self, _view: &WKWebView, _nav: Option<&AnyObject>, error: &NSError) {
            self.failed(error);
        }
    }

    impl Proxy {
        /// WebKit asks once, when the delegate is set, which calls it may make: ours and wry's.
        #[unsafe(method(respondsToSelector:))]
        fn responds(&self, sel: Sel) -> bool {
            // SAFETY: NSObject's own `respondsToSelector:`, same signature, on a live object.
            let own: bool = unsafe { msg_send![super(self), respondsToSelector: sel] };
            own || self.ivars().inner.respondsToSelector(sel)
        }

        /// Every call this class doesn't define goes to wry's delegate, as if it were still the delegate.
        #[unsafe(method(forwardingTargetForSelector:))]
        fn forward(&self, _sel: Sel) -> *mut AnyObject {
            Retained::as_ptr(&self.ivars().inner) as *mut AnyObject
        }
    }
);

impl Proxy {
    fn failed(&self, error: &NSError) {
        let url = error
            .userInfo()
            .objectForKey(&NSString::from_str("NSErrorFailingURLKey"))
            .and_then(|v| v.downcast::<NSURL>().ok())
            .and_then(|u| u.absoluteString())
            .map(|s| s.to_string());
        (self.ivars().on_fail)(LoadFailure {
            domain: error.domain().to_string(),
            code: error.code(),
            description: error.localizedDescription().to_string(),
            url,
        });
    }
}

/// Keeps the proxy alive (WKWebView holds its delegate weakly) and gives wry its delegate back when dropped.
pub struct FailureWatch {
    proxy: Retained<Proxy>,
    view: Weak<WKWebView>,
}

impl Drop for FailureWatch {
    fn drop(&mut self) {
        if let Some(view) = self.view.load() {
            // SAFETY: on the main thread (the proxy is MainThreadOnly and this owns it); wry keeps its own delegate
            // alive, and handing it back is what wry set in the first place.
            unsafe { view.setNavigationDelegate(Some(&self.proxy.ivars().inner)) };
        }
    }
}

/// Calls `on_fail` on the main thread for each main-frame load of `view` that fails (before or after it commits),
/// cancelled ones included: the caller decides what counts. `None` when `view` has no delegate to wrap.
pub fn watch_load_failures(
    view: &WKWebView,
    on_fail: impl Fn(LoadFailure) + 'static,
) -> Option<FailureWatch> {
    let mtm = MainThreadMarker::from(view);
    // SAFETY: a getter, on the main thread (the MainThreadOnly `view` proves it).
    let inner = unsafe { view.navigationDelegate() }?;
    let proxy = Proxy::alloc(mtm).set_ivars(Ivars {
        inner,
        on_fail: Box::new(on_fail),
    });
    // SAFETY: NSObject's `init` on a freshly allocated object with its ivars set.
    let proxy: Retained<Proxy> = unsafe { msg_send![super(proxy), init] };
    // SAFETY: main thread; the returned watch keeps the proxy alive for as long as it is the delegate.
    unsafe { view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*proxy))) };
    Some(FailureWatch {
        proxy,
        view: Weak::from(view),
    })
}
