//! The parts of the sign-in window that are specific to macOS.
//!
//! The cookies of the web view are read from the `WKHTTPCookieStore` of its
//! private data store. The store calls back on the main thread later, and
//! the cookies are sent when it does. The code does not run the run loop
//! until the store has called back, because that would run GPUI's code
//! from inside GPUI's code.
//!
//! The failed loads of the main frame are not reported by `wry`'s
//! navigation delegate. A delegate of this module is therefore set in its
//! place. It reports the failed loads and passes every other message on to
//! `wry`'s delegate.
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
        // SAFETY: WebKit gives the block an array that is valid for as long
        // as the call lasts.
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
    // take no arguments and return objects that belong to the view.
    // `getAllCookies:` takes a block that WebKit calls once, on the main
    // thread, which is the thread this code runs on.
    unsafe {
        let store = web_view
            .configuration()
            .websiteDataStore()
            .httpCookieStore();
        store.getAllCookies(&handler);
    }
}

/// Returns the content view of the window that holds `gpui_view`. The web
/// view is added to the content view, next to GPUI's view, and not inside
/// GPUI's view. AppKit offers a key equivalent such as ⌘V, ⌘C or ⌘A to each
/// subview of the content view in turn, and GPUI's view does not pass a key
/// equivalent on to its own subviews, so inside GPUI's view the web view
/// would not receive it.
pub fn content_view(gpui_view: NonNull<std::ffi::c_void>) -> Option<NonNull<std::ffi::c_void>> {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    // SAFETY: `gpui_view` is GPUI's NSView of an open window. `window` and
    // `contentView` take no arguments and return the window and its content
    // view. The window keeps its content view alive while it is open.
    unsafe {
        let view = gpui_view.cast::<AnyObject>().as_ref();
        let window: Option<Retained<AnyObject>> = msg_send![view, window];
        let content: Option<Retained<AnyObject>> = msg_send![&*window?, contentView];
        let content = content?;
        NonNull::new(Retained::as_ptr(&content) as *mut std::ffi::c_void)
    }
}

/// Keeps the [`FailureDelegate`] alive while the window is open. This is
/// needed because the web view holds only a weak reference to its
/// navigation delegate.
pub struct Failures {
    _delegate: Retained<FailureDelegate>,
}

/// Replaces the web view's navigation delegate, which is `wry`'s, with a
/// [`FailureDelegate`]. The new delegate sends [`Event::Failed`] when a load
/// of the main frame fails and passes every other message on to `wry`'s
/// delegate.
pub fn watch_failures(view: &wry::WebView, events: mpsc::UnboundedSender<Event>) -> Option<Failures> {
    let mtm = MainThreadMarker::new()?;
    let web_view = view.webview();
    // SAFETY: `navigationDelegate` takes no arguments and returns the web
    // view's delegate. That is `wry`'s delegate, which `wry` keeps alive for
    // as long as the web view exists.
    let inner = unsafe { web_view.navigationDelegate() };
    let delegate = FailureDelegate::new(inner, events, mtm);
    // SAFETY: `Failures` keeps the delegate alive for as long as the web
    // view can call it; the web view itself holds only a weak reference.
    // WebKit asks a delegate which methods it implements at the moment the
    // delegate is set, so this delegate is set after `wry`'s exists.
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
        /// Returns whether this delegate or `wry`'s delegate implements
        /// the method `selector`.
        #[unsafe(method(respondsToSelector:))]
        fn responds_to_selector(&self, selector: Sel) -> bool {
            // SAFETY: `respondsToSelector:` of NSObject takes a selector.
            let own: bool = unsafe { msg_send![super(self), respondsToSelector: selector] };
            own || self.ivars().inner.as_ref().is_some_and(|inner| {
                let inner: &AnyObject = inner.as_ref();
                // SAFETY: the same call as above, on `wry`'s delegate.
                unsafe { msg_send![inner, respondsToSelector: selector] }
            })
        }

        /// Returns `wry`'s delegate as the receiver of every message that
        /// this delegate does not implement.
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
        // SAFETY: `init` of NSObject is called on a new allocation.
        unsafe { msg_send![super(this), init] }
    }

    fn failed(&self, web_view: &WKWebView, error: &NSError) {
        let domain = error.domain().to_string();
        let code = error.code();
        // A load that a newer load stopped (`NSURLErrorCancelled`) and a
        // load that the window itself refused
        // (`WebKitErrorFrameLoadInterruptedByPolicyChange`) are not failures
        // of the page, so they are not reported.
        if (domain == "NSURLErrorDomain" && code == -999)
            || (domain == "WebKitErrorDomain" && code == 102)
        {
            return;
        }
        let url = failing_url(error)
            // SAFETY: `URL` takes no arguments and returns the page's address.
            .or_else(|| unsafe { web_view.URL() }.and_then(|url| url.absoluteString()).map(|url| url.to_string()))
            .unwrap_or_default();
        let _ = self.ivars().events.unbounded_send(Event::Failed {
            url,
            description: error.localizedDescription().to_string(),
        });
    }
}

/// Returns the address that could not be loaded, which the error holds
/// under the key `NSErrorFailingURLStringKey`.
fn failing_url(error: &NSError) -> Option<String> {
    let info = error.userInfo();
    let value = info.objectForKey(&NSString::from_str("NSErrorFailingURLStringKey"))?;
    let value = value.downcast::<NSString>().ok()?;
    Some(value.to_string())
}

/// Saves a picture of the window at `path` with `screencapture -l`, which
/// reads the window's content from the window server. The picture is
/// complete also when other windows cover the window, and it does not
/// include the window's shadow.
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
