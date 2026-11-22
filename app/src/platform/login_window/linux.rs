//! The sign-in window on Linux: a WebKitGTK view in a GTK window. GPUI's
//! windows on X11 and Wayland are not GTK windows, so a WebKitGTK view
//! cannot be placed inside one. The sign-in window is therefore a GTK
//! top-level window. GTK opens its own connection to the display server
//! (X11 or Wayland, whichever GTK chooses), in addition to GPUI's
//! connection. GTK is started on the main thread when the first sign-in
//! window opens. While a sign-in window is open, a GPUI task processes
//! GTK's events every 10 ms. A page that could not be loaded is reported by
//! WebKitGTK's signal `load-failed`.
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
    /// Whether GTK has destroyed the window, which happens when the user
    /// closes it.
    destroyed: Rc<std::cell::Cell<bool>>,
    _pump: Task<()>,
}

thread_local! {
    /// The result of starting GTK. It is `None` until the first sign-in
    /// window opens; GTK is started only once in a process.
    static STARTED: RefCell<Option<Result<(), String>>> = const { RefCell::new(None) };
}

fn start_gtk() -> Result<(), String> {
    STARTED.with(|started| {
        started
            .borrow_mut()
            .get_or_insert_with(|| {
                // GTK gives its windows the program's name as their
                // application id on Wayland, and the program's name and
                // class as their class on X11. Both are set to the app's id,
                // so that a desktop groups the sign-in window with the app's
                // other windows, which carry the same id
                // (`ui::window_chrome::frameless`). The name is set before
                // GTK starts. The class can be set only after GTK has
                // started, and before the first window is made.
                gtk::glib::set_prgname(Some(crate::desktop::APP_ID));
                gtk::init().map_err(|error| {
                    format!(
                        "Signing in needs GTK and WebKitGTK 4.1, which could not start: {error}"
                    )
                })?;
                gtk::gdk::set_program_class(crate::desktop::APP_ID);
                Ok(())
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
        let destroyed = Rc::new(std::cell::Cell::new(false));
        let marked = destroyed.clone();
        window.connect_destroy(move |_| {
            marked.set(true);
            super::closed();
        });
        let web_view = match super::configure(wry::WebViewBuilder::new(), &options)
            .build_gtk(&window)
        {
            Ok(web_view) => web_view,
            Err(error) => {
                // SAFETY: no other code holds this window, and it is empty.
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
            destroyed,
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
            // With `PICTO_GPUI_BACKGROUND` the window is shown without
            // the keyboard focus.
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

    /// Closes the window the way its close button does, which sends GTK's
    /// `delete-event`.
    #[cfg(debug_assertions)]
    pub fn close_as_user(&self, _: &mut App) {
        self.window.close();
    }

    /// Saves a picture of the window's content, read from the window's
    /// surface, as a PNG file at `path`.
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

    pub fn close(self, cx: &mut App) {
        self.view.borrow_mut().take();
        // GTK has already destroyed a window that the user closed, so only
        // a window that is still there is destroyed here.
        if !self.destroyed.get() {
            // SAFETY: no other code holds this window, and it is not used
            // after it is destroyed.
            unsafe { self.window.destroy() };
        }
        // GTK removes the window from the screen while it processes its
        // events. The task that processed them stops when the web view is
        // dropped, so this task processes them for another 500 ms.
        cx.spawn(async move |cx| {
            for _ in 0..50 {
                while gtk::events_pending() {
                    gtk::main_iteration_do(false);
                }
                cx.background_executor()
                    .timer(Duration::from_millis(10))
                    .await;
            }
        })
        .detach();
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
    // The cookie manager returns the cookies that the web view would send
    // with a request to `url`.
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

/// Sends [`Event::Failed`] when a load of the main frame fails (WebKitGTK's
/// signal `load-failed`). A load that a newer load stopped, or that the
/// window itself refused, is not reported. After a failed load the window
/// shows WebKitGTK's own error page.
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
