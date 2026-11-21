//! Windows: the sign-in web view's cookies, from WebView2's cookie manager,
//! handed back when it answers through the thread's messages rather than
//! by running them until it does (which would re-enter GPUI); its
//! main-frame load failures, from a `NavigationCompleted` handler of its
//! own beside `wry`'s; and the removal of the web view's user data folder
//! once its browser process has let go of it.
use super::Event;
use crate::logins::cookies::Cookie;
use futures::channel::{mpsc, oneshot};
use std::cell::RefCell;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{GetCookiesCompletedHandler, NavigationCompletedEventHandler, take_pwstr};
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

/// Reports a main-frame load that did not complete as [`Event::Failed`]:
/// one with no answer from the server (a page the server answered, a 404,
/// loads as Electron loads it), and not one stopped by a newer one or
/// refused by the window (`OPERATION_CANCELED`, Electron's `ERR_ABORTED`).
pub fn watch_failures(view: &wry::WebView, events: mpsc::UnboundedSender<Event>) {
    let handler = NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut success = BOOL::default();
        let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
        // SAFETY: the arguments are WebView2's, valid for the call; each
        // getter writes a BOOL or a status.
        unsafe {
            args.IsSuccess(&mut success)?;
            args.WebErrorStatus(&mut status)?;
        }
        if success.as_bool() || status == COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED {
            return Ok(());
        }
        if let Ok(args) = args.cast::<ICoreWebView2NavigationCompletedEventArgs2>() {
            let mut code = 0;
            // SAFETY: as above; the getter writes the HTTP status, 0 when
            // there was no answer.
            if unsafe { args.HttpStatusCode(&mut code) }.is_ok() && code != 0 {
                return Ok(());
            }
        }
        let url = sender
            .and_then(|webview| {
                let mut source = PWSTR::null();
                // SAFETY: `Source` writes a string WebView2 allocated, which
                // `take_pwstr` takes and frees.
                unsafe { webview.Source(&mut source) }.ok()?;
                Some(take_pwstr(source))
            })
            .unwrap_or_default();
        let _ = events.unbounded_send(Event::Failed {
            url,
            description: status_name(status).to_owned(),
        });
        Ok(())
    }));
    let mut token = 0;
    // SAFETY: the handler is WebView2's to call on this thread for as long
    // as the web view lives; the token is not needed, as the handler goes
    // with the web view.
    if let Err(error) = unsafe { view.webview().add_NavigationCompleted(&handler, &mut token) } {
        eprintln!("picto: the sign-in window cannot report pages that fail to load: {error}");
    }
}

/// WebView2's name for why a page did not load.
fn status_name(status: COREWEBVIEW2_WEB_ERROR_STATUS) -> &'static str {
    match status {
        COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_COMMON_NAME_IS_INCORRECT => {
            "CERTIFICATE_COMMON_NAME_IS_INCORRECT"
        }
        COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_EXPIRED => "CERTIFICATE_EXPIRED",
        COREWEBVIEW2_WEB_ERROR_STATUS_CLIENT_CERTIFICATE_CONTAINS_ERRORS => {
            "CLIENT_CERTIFICATE_CONTAINS_ERRORS"
        }
        COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_REVOKED => "CERTIFICATE_REVOKED",
        COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_IS_INVALID => "CERTIFICATE_IS_INVALID",
        COREWEBVIEW2_WEB_ERROR_STATUS_SERVER_UNREACHABLE => "SERVER_UNREACHABLE",
        COREWEBVIEW2_WEB_ERROR_STATUS_TIMEOUT => "TIMEOUT",
        COREWEBVIEW2_WEB_ERROR_STATUS_ERROR_HTTP_INVALID_SERVER_RESPONSE => {
            "HTTP_INVALID_SERVER_RESPONSE"
        }
        COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_ABORTED => "CONNECTION_ABORTED",
        COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_RESET => "CONNECTION_RESET",
        COREWEBVIEW2_WEB_ERROR_STATUS_DISCONNECTED => "DISCONNECTED",
        COREWEBVIEW2_WEB_ERROR_STATUS_CANNOT_CONNECT => "CANNOT_CONNECT",
        COREWEBVIEW2_WEB_ERROR_STATUS_HOST_NAME_NOT_RESOLVED => "HOST_NAME_NOT_RESOLVED",
        COREWEBVIEW2_WEB_ERROR_STATUS_REDIRECT_FAILED => "REDIRECT_FAILED",
        COREWEBVIEW2_WEB_ERROR_STATUS_UNEXPECTED_ERROR => "UNEXPECTED_ERROR",
        COREWEBVIEW2_WEB_ERROR_STATUS_VALID_AUTHENTICATION_CREDENTIALS_REQUIRED => {
            "VALID_AUTHENTICATION_CREDENTIALS_REQUIRED"
        }
        COREWEBVIEW2_WEB_ERROR_STATUS_VALID_PROXY_AUTHENTICATION_REQUIRED => {
            "VALID_PROXY_AUTHENTICATION_REQUIRED"
        }
        _ => "UNKNOWN",
    }
}

/// Saves a picture of the window as it is on the screen: the screen's
/// pixels inside the window's rectangle, which hold WebView2's own window
/// as the system composed it.
#[cfg(debug_assertions)]
pub fn capture(window: &gpui::Window, path: &std::path::Path) -> Result<(), String> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
        CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC,
        SRCCOPY, SelectObject,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let hwnd = crate::platform::windows::hwnd(window).ok_or("The sign-in window has no handle.")?;
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is the live window; the rectangle is written in full.
    if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
        return Err("The sign-in window has no rectangle.".into());
    }
    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    if width <= 0 || height <= 0 {
        return Err("The sign-in window has no size.".into());
    }
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    // SAFETY: the screen's device context, a memory one and a bitmap of the
    // window's size are made, used and released here; `GetDIBits` writes
    // `height` top-down rows of `width` 32-bit pixels into `pixels`, which
    // holds exactly that.
    let copied = unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let previous = SelectObject(memory, bitmap.into());
        let blitted = BitBlt(
            memory,
            0,
            0,
            width,
            height,
            Some(screen),
            rect.left,
            rect.top,
            SRCCOPY | CAPTUREBLT,
        );
        SelectObject(memory, previous);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        let lines = GetDIBits(
            memory,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        blitted.is_ok() && lines == height
    };
    if !copied {
        return Err("The screen could not be read.".into());
    }
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    image::RgbaImage::from_raw(width as u32, height as u32, pixels)
        .ok_or("The picture has the wrong size.")?
        .save(path)
        .map_err(|error| error.to_string())
}
