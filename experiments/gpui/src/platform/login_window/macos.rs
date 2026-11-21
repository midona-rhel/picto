//! macOS: the sign-in web view's cookies, from its own (private) data
//! store's `WKHTTPCookieStore`, handed back when the store answers on the
//! main thread rather than by running the run loop until it does (which
//! would re-enter GPUI); and its main-frame load failures, which `wry`'s
//! navigation delegate does not take, from a delegate in front of it that
//! passes everything else on.
use super::Event;
use crate::logins::cookies::{self, Cookie};
use futures::channel::{mpsc, oneshot};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{DefinedClass as _, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSArray, NSError, NSHTTPCookie, NSString};
use objc2_web_kit::{WKNavigation, WKNavigationDelegate, WKWebView};
use std::cell::RefCell;
use std::ptr::NonNull;
use wry::WebViewExtMacOS as _;

pub fn cookies_for(view: &wry::WebView, url: &str, sender: oneshot::Sender<Vec<Cookie>>) {
    let url = url.to_owned();
    let sender = RefCell::new(Some(sender));
    let handler = block2::RcBlock::new(move |stored: NonNull<NSArray<NSHTTPCookie>>| {
        // SAFETY: WebKit hands the block a valid array for the length of
        // the call.
        let stored = unsafe { stored.as_ref() };
        let all: Vec<Cookie> = stored
            .iter()
            .map(|cookie| Cookie {
                name: cookie.name().to_string(),
                value: cookie.value().to_string(),
                domain: cookie.domain().to_string(),
                path: cookie.path().to_string(),
                secure: cookie.isSecure(),
            })
            .collect();
        if let Some(sender) = sender.borrow_mut().take() {
            let _ = sender.send(cookies::sent_to(&all, &url));
        }
    });
    let web_view = view.webview();
    // SAFETY: `configuration`, `websiteDataStore` and `httpCookieStore`
    // take nothing and return the view's own objects; `getAllCookies:`
    // takes a block WebKit calls once, on the main thread, where this runs.
    unsafe {
        let store = web_view
            .configuration()
            .websiteDataStore()
            .httpCookieStore();
        store.getAllCookies(&handler);
    }
}

/// The window's content view, for the web view to sit in beside GPUI's
/// view rather than inside it: AppKit offers a key equivalent (⌘V, ⌘C, ⌘A)
/// to each of the content view's subviews in turn, and GPUI's view does not
/// pass it on to its own.
pub fn content_view(gpui_view: NonNull<std::ffi::c_void>) -> Option<NonNull<std::ffi::c_void>> {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    // SAFETY: `gpui_view` is GPUI's live NSView of an open window; `window`
    // and `contentView` take nothing and return the window and its content
    // view, which the window keeps alive while it is open.
    unsafe {
        let view = gpui_view.cast::<AnyObject>().as_ref();
        let window: Option<Retained<AnyObject>> = msg_send![view, window];
        let content: Option<Retained<AnyObject>> = msg_send![&*window?, contentView];
        let content = content?;
        NonNull::new(Retained::as_ptr(&content) as *mut std::ffi::c_void)
    }
}

/// The delegate in front of `wry`'s, kept while the window is open (the
/// web view holds its delegate weakly).
pub struct Failures {
    _delegate: Retained<FailureDelegate>,
}

/// Puts a delegate in front of the web view's navigation delegate that
/// reports a main-frame load failure as [`Event::Failed`] and hands every
/// other message to `wry`'s.
pub fn watch_failures(view: &wry::WebView, events: mpsc::UnboundedSender<Event>) -> Option<Failures> {
    let mtm = MainThreadMarker::new()?;
    let web_view = view.webview();
    // SAFETY: `navigationDelegate` takes nothing and returns the web view's
    // delegate, `wry`'s, which `wry` keeps alive with the web view.
    let inner = unsafe { web_view.navigationDelegate() };
    let delegate = FailureDelegate::new(inner, events, mtm);
    // SAFETY: the delegate is kept by `Failures` for as long as the web view
    // can call it; the web view holds it weakly. WebKit asks which methods
    // it answers when it is set, so it is set after `wry`'s.
    unsafe { web_view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
    Some(Failures {
        _delegate: delegate,
    })
}

pub struct FailureIvars {
    inner: Option<Retained<ProtocolObject<dyn WKNavigationDelegate>>>,
    events: mpsc::UnboundedSender<Event>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = FailureIvars]
    pub struct FailureDelegate;

    unsafe impl NSObjectProtocol for FailureDelegate {}

    impl FailureDelegate {
        /// What this delegate answers, and what `wry`'s does.
        #[unsafe(method(respondsToSelector:))]
        fn responds_to_selector(&self, selector: Sel) -> bool {
            // SAFETY: NSObject's `respondsToSelector:` takes a selector.
            let own: bool = unsafe { msg_send![super(self), respondsToSelector: selector] };
            own || self.ivars().inner.as_ref().is_some_and(|inner| {
                let inner: &AnyObject = inner.as_ref();
                // SAFETY: as above, on `wry`'s delegate.
                unsafe { msg_send![inner, respondsToSelector: selector] }
            })
        }

        /// Every message this delegate does not answer goes to `wry`'s.
        #[unsafe(method(forwardingTargetForSelector:))]
        fn forwarding_target(&self, _selector: Sel) -> *mut AnyObject {
            self.ivars()
                .inner
                .as_ref()
                .map_or(std::ptr::null_mut(), |inner| {
                    Retained::as_ptr(inner).cast::<AnyObject>().cast_mut()
                })
        }
    }

    unsafe impl WKNavigationDelegate for FailureDelegate {
        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail_provisional_navigation(
            &self,
            web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
            error: &NSError,
        ) {
            self.failed(web_view, error);
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail_navigation(
            &self,
            web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
            error: &NSError,
        ) {
            self.failed(web_view, error);
        }
    }
);

impl FailureDelegate {
    fn new(
        inner: Option<Retained<ProtocolObject<dyn WKNavigationDelegate>>>,
        events: mpsc::UnboundedSender<Event>,
        mtm: MainThreadMarker,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(FailureIvars { inner, events });
        // SAFETY: NSObject's `init` on a fresh allocation.
        unsafe { msg_send![super(this), init] }
    }

    fn failed(&self, web_view: &WKWebView, error: &NSError) {
        let domain = error.domain().to_string();
        let code = error.code();
        // A load stopped by a newer one (`NSURLErrorCancelled`), or one the
        // window refused (`WebKitErrorFrameLoadInterruptedByPolicyChange`),
        // as Electron leaves out `ERR_ABORTED`.
        if (domain == "NSURLErrorDomain" && code == -999)
            || (domain == "WebKitErrorDomain" && code == 102)
        {
            return;
        }
        let url = failing_url(error)
            // SAFETY: `URL` takes nothing and returns the page's address.
            .or_else(|| unsafe { web_view.URL() }.and_then(|url| url.absoluteString()).map(|url| url.to_string()))
            .unwrap_or_default();
        let _ = self.ivars().events.unbounded_send(Event::Failed {
            url,
            description: error.localizedDescription().to_string(),
        });
    }
}

/// The address that failed (`NSURLErrorFailingURLStringErrorKey`).
fn failing_url(error: &NSError) -> Option<String> {
    let info = error.userInfo();
    let value = info.objectForKey(&NSString::from_str("NSErrorFailingURLStringKey"))?;
    let value = value.downcast::<NSString>().ok()?;
    Some(value.to_string())
}

/// Saves a picture of the window as the window server has it, behind other
/// windows or not (`screencapture -l`), without its shadow.
#[cfg(debug_assertions)]
pub fn capture(window: &gpui::Window, path: &std::path::Path) -> Result<(), String> {
    let mut number = None;
    crate::platform::macos::with_ns_window(window, |ns_window| {
        number = Some(crate::platform::macos::window_number(ns_window));
    });
    let number = number.ok_or("The sign-in window has no window number.")?;
    let status = std::process::Command::new("/usr/sbin/screencapture")
        .args(["-x", "-o", "-l", &number.to_string()])
        .arg(path)
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("screencapture ended with {status}"))
    }
}
