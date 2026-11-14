//! Windows: the sign-in web view's cookies, from WebView2's cookie manager,
//! handed back when it answers through the thread's messages rather than
//! by running them until it does (which would re-enter GPUI); and the
//! removal of the web view's user data folder once its browser process has
//! let go of it.
use crate::logins::cookies::Cookie;
use futures::channel::oneshot;
use std::cell::RefCell;
use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2_2, ICoreWebView2Cookie};
use webview2_com::{GetCookiesCompletedHandler, take_pwstr};
use windows::core::{BOOL, HSTRING, Interface as _, PWSTR};
use wry::WebViewExtWindows as _;

pub fn cookies_for(view: &wry::WebView, url: &str, sender: oneshot::Sender<Vec<Cookie>>) {
    let sender = RefCell::new(Some(sender));
    let reply = move |cookies: Vec<Cookie>| {
        if let Some(sender) = sender.borrow_mut().take() {
            let _ = sender.send(cookies);
        }
    };
    let manager = view
        .webview()
        .cast::<ICoreWebView2_2>()
        .and_then(|webview| unsafe { webview.CookieManager() });
    let Ok(manager) = manager else {
        reply(Vec::new());
        return;
    };
    let reply = std::rc::Rc::new(reply);
    let answer = reply.clone();
    let handler = GetCookiesCompletedHandler::create(Box::new(move |result, list| {
        let mut cookies = Vec::new();
        if let (Ok(()), Some(list)) = (result, list) {
            let mut count = 0;
            // SAFETY: the list and its cookies are WebView2's, valid for the
            // call.
            unsafe {
                if list.Count(&mut count).is_ok() {
                    for index in 0..count {
                        if let Ok(cookie) = list.GetValueAtIndex(index) {
                            if let Some(cookie) = read(&cookie) {
                                cookies.push(cookie);
                            }
                        }
                    }
                }
            }
        }
        answer(cookies);
        Ok(())
    }));
    // SAFETY: `GetCookies` takes the address (cookies sent to it, as
    // Electron's `cookies.get({ url })` answers) and a handler WebView2
    // calls once on this thread.
    if unsafe { manager.GetCookies(&HSTRING::from(url), &handler) }.is_err() {
        reply(Vec::new());
    }
}

/// One cookie's fields.
///
/// # Safety
/// `cookie` is WebView2's, valid for the call.
unsafe fn read(cookie: &ICoreWebView2Cookie) -> Option<Cookie> {
    // SAFETY: each getter writes a string WebView2 allocated, which
    // `take_pwstr` takes and frees, or a BOOL.
    unsafe {
        let mut name = PWSTR::null();
        cookie.Name(&mut name).ok()?;
        let mut value = PWSTR::null();
        cookie.Value(&mut value).ok()?;
        let mut domain = PWSTR::null();
        cookie.Domain(&mut domain).ok()?;
        let mut path = PWSTR::null();
        cookie.Path(&mut path).ok()?;
        let mut secure: BOOL = false.into();
        cookie.IsSecure(&mut secure).ok()?;
        Some(Cookie {
            name: take_pwstr(name),
            value: take_pwstr(value),
            domain: take_pwstr(domain),
            path: take_pwstr(path),
            secure: secure.as_bool(),
        })
    }
}

/// Removes a sign-in window's user data folder once WebView2's browser
/// process, which outlives the window by a moment, has let go of it.
pub fn remove_later(data: tempfile::TempDir) {
    let path = data.keep();
    std::thread::spawn(move || {
        for _ in 0..20 {
            if std::fs::remove_dir_all(&path).is_ok() || !path.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });
}
