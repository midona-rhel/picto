//! The sign-in window. It is a window of its own that shows a service's
//! login page in the web view of the operating system. The app reads the
//! cookies of the login from this web view, and only for the addresses of
//! that service.
//!
//! [`LoginWindow`] is the same on every platform. The web view is placed
//! differently on each one:
//!
//! - On macOS it is a WKWebView inside a GPUI window (`hosted.rs`). Its
//!   cookies are read from the view's `WKHTTPCookieStore` (`macos.rs`).
//! - On Windows it is a WebView2 inside a GPUI window (`hosted.rs`). Its
//!   cookies are read from `ICoreWebView2CookieManager` (`windows.rs`). The
//!   WebView2 Runtime must be installed.
//! - On Linux, with X11 and with Wayland, it is a WebKitGTK view in a GTK
//!   window (`linux.rs`), because a GPUI window on Linux is not a GTK
//!   window and cannot hold one. GTK runs on the main thread, and the app
//!   processes GTK's events while a sign-in window is open. WebKitGTK 4.1
//!   must be installed.
//!
//! Every web view is made with a private data store: its storage starts
//! empty, no other window shares it, and it is deleted when the window
//! closes. A login therefore never starts with the cookies of an earlier
//! login. Nothing here reads the cookies of a browser that is installed on
//! the computer.
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

/// An event that the sign-in window reports to the code that opened it.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The page at this address started to load.
    Started(String),
    /// The page at this address finished loading.
    Loaded(String),
    /// The title of the page changed to this text.
    Title(String),
    /// The window was about to load this address, which is the callback of
    /// the login (`Options::intercept`). The window did not load it.
    Callback(String),
    /// A page asked to open this address in a new window. No new window is
    /// opened; the code that receives the event loads the address in this
    /// window.
    OpenHere(String),
    /// The page of the window's main frame could not be loaded. The event
    /// holds the address and the system's description of the reason. A
    /// load that a newer load stopped, or that the window itself refused,
    /// is not reported.
    Failed { url: String, description: String },
    /// The web view could not be made after its window had opened, for
    /// example because the WebView2 Runtime is not installed. The code that
    /// receives the event closes the window. On Linux the web view is made
    /// together with its window, so there `open` returns the error and
    /// this event is not sent.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    Unavailable(String),
    /// The user closed the window.
    Closed,
}

/// The values that a sign-in window is opened with.
pub struct Options {
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub min_width: f32,
    pub min_height: f32,
    /// Returns whether an address is the callback of the login. The window
    /// does not load such an address and sends [`Event::Callback`] with it
    /// instead.
    pub intercept: Rc<dyn Fn(&str) -> bool>,
    pub events: mpsc::UnboundedSender<Event>,
    /// The loopback origin of a check in a debug build:
    /// `PICTO_GPUI_LOGIN_ORIGIN` for a sign-in, `PICTO_GPUI_NET_ORIGIN` for
    /// Search by Image. When it is set, the window loads no address outside
    /// that origin. It is `None` when the app runs normally.
    pub local_origin: Option<String>,
}

/// An open sign-in window. It is hidden until [`LoginWindow::show`] is
/// called.
pub struct LoginWindow {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    inner: hosted::Hosted,
    #[cfg(target_os = "linux")]
    inner: linux::GtkLogin,
}

impl LoginWindow {
    /// Opens the window without showing it. Returns an error if the system
    /// could not open it.
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
        self.inner.load(url);
    }

    /// Returns the address of the page that the window shows.
    pub fn current_url(&self) -> String {
        self.inner
            .with_view(|view| view.url().unwrap_or_default())
            .unwrap_or_default()
    }

    /// Returns the cookies that the web view would send with a request to
    /// `url`. They come from the web view's own cookie store.
    pub fn cookies(&self, url: &str) -> oneshot::Receiver<Vec<Cookie>> {
        let (sender, receiver) = oneshot::channel();
        self.inner.with_view(|view| cookies_for(view, url, sender));
        receiver
    }

    /// Runs `script` in the page. `script` is a JavaScript expression whose
    /// value is a JSON string. Returns the parsed JSON, or `None` if the
    /// result is not a JSON string.
    pub fn eval(&self, script: &str) -> oneshot::Receiver<Option<Value>> {
        let (sender, receiver) = oneshot::channel();
        let sender = std::sync::Mutex::new(Some(sender));
        self.inner.with_view(|view| {
            let result = view.evaluate_script_with_callback(script, move |json| {
                // The web view returns the script's value encoded as JSON.
                // The value is itself a JSON string, so it is decoded twice.
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

    /// Brings the window to the front and gives it the keyboard focus,
    /// unless `PICTO_GPUI_BACKGROUND` keeps every window of the app behind
    /// the windows of other programs.
    pub fn focus(&self, cx: &mut App) {
        self.inner.focus(cx);
    }

    /// Closes the window. The storage of its web view is deleted with it.
    pub fn close(self, cx: &mut App) {
        self.inner.close(cx);
    }

    /// Closes the window the way its close button does. Only the check in
    /// `logins::check` uses it.
    #[cfg(debug_assertions)]
    pub fn close_as_user(&self, cx: &mut App) {
        self.inner.close_as_user(cx);
    }

    /// Saves a picture of the window, as it is on the screen, as a PNG file
    /// at `path`. Only the checks use it.
    #[cfg(debug_assertions)]
    pub fn capture(&self, path: &std::path::Path, cx: &mut App) -> Result<(), String> {
        self.inner.capture(path, cx)
    }

    /// Returns the number of sign-in windows that are open. Only the
    /// checks use it.
    #[cfg(debug_assertions)]
    pub fn open_count() -> usize {
        OPEN.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// The number of sign-in windows that are open.
static OPEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Adds one to [`OPEN`]. Each platform calls it when it has made a window.
fn opened() {
    OPEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

/// Takes one from [`OPEN`]. Each platform calls it when a window has been
/// destroyed.
fn closed() {
    let _ = OPEN.fetch_update(
        std::sync::atomic::Ordering::SeqCst,
        std::sync::atomic::Ordering::SeqCst,
        |open| open.checked_sub(1),
    );
}

fn load_into(view: &wry::WebView, url: &str) {
    if let Err(error) = view.load_url(url) {
        eprintln!("picto: the sign-in window could not open a page: {error}");
    }
}

/// Sets up the web view the same way on every platform: a private data
/// store, the handlers that send the [`Event`]s, no access to the clipboard
/// for page scripts, no new windows, and every permission request denied.
fn configure<'a>(builder: wry::WebViewBuilder<'a>, options: &Options) -> wry::WebViewBuilder<'a> {
    let intercept = options.intercept.clone();
    let local = options.local_origin.clone();
    let local_for_windows = options.local_origin.clone();
    let events = options.events.clone();
    let started = options.events.clone();
    let titled = options.events.clone();
    let opened = options.events.clone();
    builder
        .with_incognito(true)
        .with_visible(true)
        .with_background_color((255, 255, 255, 255))
        // A script of the page cannot read or write the clipboard (on
        // WebKitGTK this is the setting `javascript-can-access-clipboard`).
        // The user can still copy and paste in the fields of the page,
        // because the web view does that itself.
        .with_clipboard(false)
        .with_navigation_handler(move |url| {
            if intercept(&url) {
                let _ = events.unbounded_send(Event::Callback(url));
                return false;
            }
            allowed(&url, local.as_deref())
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
        // A page never gets a new window. When it asks for one with an
        // HTTPS address, or with an address of the check's local origin,
        // the address is loaded in this window instead.
        .with_new_window_req_handler(move |url, _| {
            if url.to_ascii_lowercase().starts_with("https://")
                || under_local_origin(&url, local_for_windows.as_deref())
            {
                let _ = opened.unbounded_send(Event::OpenHere(url));
            }
            wry::NewWindowResponse::Deny
        })
        // Every permission request of a page is denied: camera,
        // microphone, location, notifications and all others. A login needs
        // none of them.
        .with_permission_handler(|_| wry::PermissionResponse::Deny)
}

/// Returns whether the window may load `url`. When `local` holds the
/// loopback origin of a check (a debug build's local login or local search
/// engines), only `about:blank` and addresses of that origin may be loaded,
/// so a check never reaches a real site.
fn allowed(url: &str, local: Option<&str>) -> bool {
    match local {
        Some(_) if url == "about:blank" => true,
        Some(_) if !under_local_origin(url, local) => {
            eprintln!("picto: the web view refused {url} (outside the local origin)");
            false
        }
        _ => true,
    }
}

fn under_local_origin(url: &str, local: Option<&str>) -> bool {
    local.is_some_and(|origin| {
        url::Url::parse(url)
            .ok()
            .zip(url::Url::parse(origin).ok())
            .is_some_and(|(url, origin)| url.origin() == origin.origin())
    })
}

/// Shows the GPUI window that holds the web view without giving it the
/// keyboard focus, or hides it. With `PICTO_GPUI_BACKGROUND` the window is
/// shown behind every other window.
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

/// Reads the cookies that the web view would send with a request to `url`
/// and sends them through `sender`. It does not wait: the cookie store
/// calls back on the main thread later.
fn cookies_for(view: &wry::WebView, url: &str, sender: oneshot::Sender<Vec<Cookie>>) {
    #[cfg(target_os = "macos")]
    macos::cookies_for(view, url, sender);
    #[cfg(target_os = "windows")]
    windows::cookies_for(view, url, sender);
    #[cfg(target_os = "linux")]
    linux::cookies_for(view, url, sender);
}
