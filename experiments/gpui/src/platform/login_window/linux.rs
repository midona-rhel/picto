//! Linux: the sign-in web view in a GTK window of its own. GPUI's X11 and
//! Wayland windows are not GTK's, so WebKitGTK cannot sit inside one; a
//! GTK top-level window opens its own connection to the display server
//! (X11 or Wayland, whichever GTK finds), beside GPUI's. GTK is started on
//! the main thread when the first sign-in window opens, and its events are
//! run from GPUI's loop every 10 ms while a sign-in window is open. A page
//! that fails to load is WebKitGTK's `load-failed`.
use super::{Event, Options};
use crate::logins::cookies::Cookie;
use futures::channel::oneshot;
use gpui::{App, Task};
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use webkit2gtk::{CookieManagerExt as _, WebViewExt as _, WebsiteDataManagerExt as _};
use wry::{WebViewBuilderExtUnix as _, WebViewExtUnix as _};

type Slot = Rc<RefCell<Option<wry::WebView>>>;

pub struct GtkLogin {
    window: gtk::Window,
    view: Slot,
    _pump: Task<()>,
}

thread_local! {
    /// Whether GTK started, once per process.
    static STARTED: RefCell<Option<Result<(), String>>> = const { RefCell::new(None) };
}

fn start_gtk() -> Result<(), String> {
    STARTED.with(|started| {
        started
            .borrow_mut()
            .get_or_insert_with(|| {
                gtk::init().map_err(|error| {
                    format!(
                        "Signing in needs GTK and WebKitGTK 4.1, which could not start: {error}"
                    )
                })
            })
            .clone()
    })
}

impl GtkLogin {
    pub fn open(options: Options, cx: &mut App) -> Result<Self, String> {
        start_gtk()?;
        let view: Slot = Rc::new(RefCell::new(None));
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title(&options.title);
        window.set_default_size(options.width as i32, options.height as i32);
        window.set_size_request(options.min_width as i32, options.min_height as i32);
        window.set_position(gtk::WindowPosition::Center);
        let closed = options.events.clone();
        let slot = view.clone();
        window.connect_delete_event(move |_, _| {
            slot.borrow_mut().take();
            let _ = closed.unbounded_send(Event::Closed);
            gtk::glib::Propagation::Proceed
        });
        super::opened();
        window.connect_destroy(|_| super::closed());
        let web_view = match super::configure(wry::WebViewBuilder::new(), &options)
            .build_gtk(&window)
        {
            Ok(web_view) => web_view,
            Err(error) => {
                // SAFETY: the window is this login's own and holds nothing.
                unsafe { window.destroy() };
                return Err(format!("The sign-in window could not open: {error}"));
            }
        };
        watch_failures(&web_view, options.events.clone());
        *view.borrow_mut() = Some(web_view);
        let alive = Rc::downgrade(&view);
        let pump = cx.spawn(async move |cx| {
            while alive.upgrade().is_some() {
                while gtk::events_pending() {
                    gtk::main_iteration_do(false);
                }
                cx.background_executor()
                    .timer(Duration::from_millis(10))
                    .await;
            }
        });
        Ok(Self {
            window,
            view,
            _pump: pump,
        })
    }

    pub fn with_view<R>(&self, work: impl FnOnce(&wry::WebView) -> R) -> Option<R> {
        self.view.borrow().as_ref().map(work)
    }

    pub fn load(&self, url: &str) {
        self.with_view(|view| super::load_into(view, url));
    }

    pub fn set_shown(&self, shown: bool, _: &mut App) {
        if shown {
            // `PICTO_GPUI_BACKGROUND`: shown without taking the keyboard.
            self.window.set_focus_on_map(!crate::seams::background());
            self.window.show_all();
        } else {
            self.window.hide();
        }
    }

    pub fn focus(&self, _: &mut App) {
        if !crate::seams::background() {
            self.window.present();
        }
    }

    /// Asks the window to close as its close button does (`delete-event`).
    #[cfg(debug_assertions)]
    pub fn close_as_user(&self, _: &mut App) {
        self.window.close();
    }

    /// Saves a picture of the window's content as the display server has
    /// it.
    #[cfg(debug_assertions)]
    pub fn capture(&self, path: &std::path::Path, _: &mut App) -> Result<(), String> {
        let surface = self
            .window
            .window()
            .ok_or("The sign-in window is not on the screen.")?;
        let (width, height) = (surface.width(), surface.height());
        let picture = surface
            .pixbuf(0, 0, width, height)
            .ok_or("The sign-in window could not be read.")?;
        picture
            .savev(path, "png", &[])
            .map_err(|error| error.to_string())
    }

    pub fn close(self, _: &mut App) {
        self.view.borrow_mut().take();
        // SAFETY: the window is this login's own, and nothing holds it
        // after it is destroyed.
        unsafe { self.window.destroy() };
    }
}

pub fn cookies_for(view: &wry::WebView, url: &str, sender: oneshot::Sender<Vec<Cookie>>) {
    let manager = view
        .webview()
        .website_data_manager()
        .and_then(|manager| manager.cookie_manager());
    let Some(manager) = manager else {
        let _ = sender.send(Vec::new());
        return;
    };
    // The cookies a request to `url` would carry, as Electron's
    // `cookies.get({ url })` answers.
    manager.cookies(url, None::<&gtk::gio::Cancellable>, move |result| {
        let cookies = result
            .map(|stored| {
                stored
                    .into_iter()
                    .filter_map(|mut cookie| {
                        Some(Cookie {
                            name: cookie.name()?.to_string(),
                            value: cookie.value()?.to_string(),
                            domain: cookie.domain().map(|d| d.to_string()).unwrap_or_default(),
                            path: cookie.path().map(|p| p.to_string()).unwrap_or_default(),
                            secure: cookie.is_secure(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let _ = sender.send(cookies);
    });
}

/// Reports a main-frame load that failed (`load-failed`) as
/// [`Event::Failed`], but not one stopped by a newer one or refused by the
/// window (Electron's `ERR_ABORTED`). The window then shows WebKitGTK's
/// own error page.
fn watch_failures(view: &wry::WebView, events: futures::channel::mpsc::UnboundedSender<Event>) {
    view.webview()
        .connect_load_failed(move |_, _, url, error| {
            let stopped = error.matches(webkit2gtk::NetworkError::Cancelled)
                || error.matches(webkit2gtk::PolicyError::FrameLoadInterruptedByPolicyChange);
            if !stopped {
                let _ = events.unbounded_send(Event::Failed {
                    url: url.to_owned(),
                    description: error.message().to_owned(),
                });
            }
            false
        });
}
