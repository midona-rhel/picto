//! The sign-in window (production's `BrowserWindow` in
//! `electron/windows/authSessions.mjs`): the service's own login page in
//! the system's web view, in a window of its own, from which the login's
//! cookies are read for that service only.
//!
//! One interface for every platform; each places the web view where it can
//! live beside GPUI's windows:
//!
//! - macOS: a WKWebView inside a GPUI window of its own (`hosted.rs`), its
//!   cookies from the view's `WKHTTPCookieStore` (`macos.rs`);
//! - Windows: a WebView2 inside a GPUI window of its own (`hosted.rs`), its
//!   cookies from `ICoreWebView2CookieManager` (`windows.rs`); it needs the
//!   WebView2 Runtime;
//! - Linux (X11 and Wayland): a GTK window holding WebKitGTK (`linux.rs`),
//!   as GPUI's Linux windows are not GTK's; GTK runs on the main thread,
//!   advanced by the app while a sign-in window is open; it needs
//!   WebKitGTK 4.1.
//!
//! Every web view starts with storage of its own that nothing else shares
//! and that goes when it closes (a private data store), which is what
//! production's partition, cleared when a login starts and when it is
//! saved, comes to. Nothing here reads another browser's cookies.
use crate::logins::cookies::Cookie;
use futures::channel::{mpsc, oneshot};
use gpui::App;
use serde_json::Value;
use std::rc::Rc;

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod hosted;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// What the window reports to the login driving it.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A page started loading (`did-navigate`).
    Started(String),
    /// A page finished loading (`did-finish-load`).
    Loaded(String),
    /// The page's title changed.
    Title(String),
    /// The window was about to go to the login's callback, which it did
    /// not load (`will-navigate`, `will-redirect`).
    Callback(String),
    /// A page asked for a new window; it opens in this one instead.
    OpenHere(String),
    /// The user closed the window.
    Closed,
}

/// What a sign-in window opens with.
pub struct Options {
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub min_width: f32,
    pub min_height: f32,
    /// Whether an address is the login's callback, which is handed back as
    /// [`Event::Callback`] instead of loading.
    pub intercept: Rc<dyn Fn(&str) -> bool>,
    pub events: mpsc::UnboundedSender<Event>,
}

/// An open sign-in window, hidden until it is shown.
pub struct LoginWindow {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    inner: hosted::Hosted,
    #[cfg(target_os = "linux")]
    inner: linux::GtkLogin,
}

impl LoginWindow {
    /// Opens the window, hidden, or says why the system could not.
    pub fn open(options: Options, cx: &mut App) -> Result<Self, String> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        return hosted::Hosted::open(options, cx).map(|inner| Self { inner });
        #[cfg(target_os = "linux")]
        return linux::GtkLogin::open(options, cx).map(|inner| Self { inner });
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            let _ = (options, cx);
            Err("Signing in is not available on this system.".into())
        }
    }

    pub fn load(&self, url: &str) {
        self.inner.with_view(|view| {
            if let Err(error) = view.load_url(url) {
                eprintln!("picto: the sign-in window could not open a page: {error}");
            }
        });
    }

    /// The page's address now.
    pub fn current_url(&self) -> String {
        self.inner
            .with_view(|view| view.url().unwrap_or_default())
            .unwrap_or_default()
    }

    /// The cookies a request to `url` carries, from the window's own store.
    pub fn cookies(&self, url: &str) -> oneshot::Receiver<Vec<Cookie>> {
        let (sender, receiver) = oneshot::channel();
        self.inner.with_view(|view| cookies_for(view, url, sender));
        receiver
    }

    /// Runs `script`, an expression whose value is a JSON string, and
    /// hands back that JSON.
    pub fn eval(&self, script: &str) -> oneshot::Receiver<Option<Value>> {
        let (sender, receiver) = oneshot::channel();
        let sender = std::sync::Mutex::new(Some(sender));
        self.inner.with_view(|view| {
            let result = view.evaluate_script_with_callback(script, move |json| {
                // The script's value is a JSON string, handed back as the
                // JSON of that string.
                let value =
                    serde_json::from_str::<Value>(&json)
                        .ok()
                        .and_then(|outer| match outer {
                            Value::String(inner) => serde_json::from_str(&inner).ok(),
                            _ => None,
                        });
                if let Some(sender) = sender.lock().ok().and_then(|mut slot| slot.take()) {
                    let _ = sender.send(value);
                }
            });
            if let Err(error) = result {
                eprintln!("picto: the sign-in window could not run a script: {error}");
            }
        });
        receiver
    }

    pub fn show(&self, cx: &mut App) {
        self.inner.set_shown(true, cx);
    }

    pub fn hide(&self, cx: &mut App) {
        self.inner.set_shown(false, cx);
    }

    /// Brings the window forward with the keyboard (`focus()`), unless
    /// `PICTO_GPUI_BACKGROUND` keeps every window of the app behind.
    pub fn focus(&self, cx: &mut App) {
        self.inner.focus(cx);
    }

    /// Closes the window; its web view's storage goes with it.
    pub fn close(self, cx: &mut App) {
        self.inner.close(cx);
    }
}

/// The web view every platform opens: the login page, nothing allowed
/// that a login does not need, and its storage its own.
fn configure<'a>(builder: wry::WebViewBuilder<'a>, options: &Options) -> wry::WebViewBuilder<'a> {
    let intercept = options.intercept.clone();
    let events = options.events.clone();
    let started = options.events.clone();
    let titled = options.events.clone();
    let opened = options.events.clone();
    builder
        .with_incognito(true)
        .with_visible(true)
        .with_background_color((255, 255, 255, 255))
        .with_clipboard(true)
        .with_navigation_handler(move |url| {
            if intercept(&url) {
                let _ = events.unbounded_send(Event::Callback(url));
                return false;
            }
            allowed(&url)
        })
        .with_on_page_load_handler(move |event, url| {
            let _ = started.unbounded_send(match event {
                wry::PageLoadEvent::Started => Event::Started(url),
                wry::PageLoadEvent::Finished => Event::Loaded(url),
            });
        })
        .with_document_title_changed_handler(move |title| {
            let _ = titled.unbounded_send(Event::Title(title));
        })
        // `setWindowOpenHandler`: a page's new window opens in this one
        // when it is HTTPS, and never as a window of its own.
        .with_new_window_req_handler(move |url, _| {
            if url.to_ascii_lowercase().starts_with("https://") || under_local_origin(&url) {
                let _ = opened.unbounded_send(Event::OpenHere(url));
            }
            wry::NewWindowResponse::Deny
        })
        // Production allows only the storage access a login's own frames
        // ask for; the web views here have no such request to answer, and
        // nothing else (camera, microphone, location, notifications).
        .with_permission_handler(|_| wry::PermissionResponse::Deny)
}

/// Whether the window may load `url`. With `PICTO_GPUI_LOGIN_ORIGIN` set (a
/// debug build's local login), nothing outside that origin loads, so a
/// check never reaches a real site.
fn allowed(url: &str) -> bool {
    match crate::seams::login_origin() {
        Some(_) if url == "about:blank" => true,
        Some(_) if !under_local_origin(url) => {
            eprintln!("picto: the sign-in window refused {url} (outside the local login)");
            false
        }
        _ => true,
    }
}

fn under_local_origin(url: &str) -> bool {
    crate::seams::login_origin().is_some_and(|origin| {
        url::Url::parse(url)
            .ok()
            .zip(url::Url::parse(&origin).ok())
            .is_some_and(|(url, origin)| url.origin() == origin.origin())
    })
}

/// Shows a hosted window without taking the keyboard, or takes it off the
/// screen. With `PICTO_GPUI_BACKGROUND` it is shown behind every other
/// window.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn show_hosted(window: &gpui::Window, shown: bool) {
    #[cfg(target_os = "macos")]
    crate::platform::macos::with_ns_window(window, |ns_window| {
        if !shown {
            crate::platform::macos::order_out(ns_window);
        } else if crate::seams::background() {
            crate::platform::macos::order_back(ns_window);
        } else {
            crate::platform::macos::order_front(ns_window);
        }
    });
    #[cfg(target_os = "windows")]
    crate::platform::windows::show_without_activating(window, shown);
}

/// The window's cookies for `url`, read without blocking: the store
/// answers on the main thread later.
fn cookies_for(view: &wry::WebView, url: &str, sender: oneshot::Sender<Vec<Cookie>>) {
    #[cfg(target_os = "macos")]
    macos::cookies_for(view, url, sender);
    #[cfg(target_os = "windows")]
    windows::cookies_for(view, url, sender);
    #[cfg(target_os = "linux")]
    linux::cookies_for(view, url, sender);
}
