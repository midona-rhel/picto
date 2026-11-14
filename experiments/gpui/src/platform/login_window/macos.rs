//! macOS: the sign-in web view's cookies, from its own (private) data
//! store's `WKHTTPCookieStore`, handed back when the store answers on the
//! main thread rather than by running the run loop until it does (which
//! would re-enter GPUI).
use crate::logins::cookies::{self, Cookie};
use futures::channel::oneshot;
use objc2_foundation::{NSArray, NSHTTPCookie};
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
