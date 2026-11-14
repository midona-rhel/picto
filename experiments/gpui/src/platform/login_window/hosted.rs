//! macOS and Windows: the web view as a child of a GPUI window of its own,
//! with the system's title bar ("Login: <service>"), white beneath the page
//! as production's window is, filling the window as it is resized.
//!
//! The web view is made from a task of its own, outside any GPUI update:
//! WebView2 waits for its environment and controller by running the
//! thread's messages, which run GPUI's own tasks and window events; those
//! borrow the app, which must not be borrowed already. A page asked for
//! before the web view is there is loaded once it is.
use super::{Event, Options};
use gpui::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wry::raw_window_handle as rwh;

type Slot = Rc<RefCell<Option<wry::WebView>>>;

pub struct Hosted {
    handle: WindowHandle<Host>,
    view: Slot,
    /// The page to load once the web view is made.
    pending: Rc<RefCell<Option<String>>>,
    /// The window has closed (the user, or the login); a web view made
    /// after that is dropped at once.
    gone: Rc<Cell<bool>>,
    /// What watches the web view's main-frame load failures, kept as long
    /// as the window.
    #[cfg(target_os = "macos")]
    failures: Rc<RefCell<Option<super::macos::Failures>>>,
    /// WebView2's user data, a fresh folder per window, removed with it.
    #[cfg(target_os = "windows")]
    data: Option<tempfile::TempDir>,
    _build: Task<()>,
}

/// The GPUI view the web view sits in.
pub struct Host {
    view: Slot,
    /// The size the web view was last given, in screen points.
    placed: Cell<Option<(f32, f32)>>,
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // The web view fills the window's content in the system's points,
        // whatever the interface zoom (GPUI lays the window out divided by
        // it).
        let size = content_size(window);
        if self.placed.get() != Some(size) {
            if let Some(view) = self.view.borrow().as_ref() {
                let _ = view.set_bounds(bounds(size));
                self.placed.set(Some(size));
            }
        }
        div()
            .id("login-window")
            .a11y_presentational()
            .size_full()
            .bg(rgb(0xffffff))
    }
}

fn content_size(window: &Window) -> (f32, f32) {
    let viewport = window.viewport_size();
    let zoom = window.ui_zoom();
    (
        f32::from(viewport.width) * zoom,
        f32::from(viewport.height) * zoom,
    )
}

fn bounds((width, height): (f32, f32)) -> wry::Rect {
    wry::Rect {
        position: wry::dpi::LogicalPosition::new(0., 0.).into(),
        size: wry::dpi::LogicalSize::new(f64::from(width), f64::from(height)).into(),
    }
}

/// A window's handle, kept to build the web view outside GPUI's update.
struct Parent(rwh::RawWindowHandle);

impl rwh::HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        // SAFETY: the handle is of a window that is checked to be open
        // before the web view is built from it, on the same thread.
        Ok(unsafe { rwh::WindowHandle::borrow_raw(self.0) })
    }
}

impl Hosted {
    pub fn open(options: Options, cx: &mut App) -> Result<Self, String> {
        let view: Slot = Rc::new(RefCell::new(None));
        let gone = Rc::new(Cell::new(false));
        let mut bounds_ = Bounds::centered(None, size(px(options.width), px(options.height)), cx);
        bounds_.origin = bounds_.origin.map(|at| at.floor());
        let closed = options.events.clone();
        let slot = view.clone();
        let gone_on_close = gone.clone();
        let handle = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds_)),
                    window_min_size: Some(size(px(options.min_width), px(options.min_height))),
                    titlebar: Some(TitlebarOptions {
                        title: Some(options.title.clone().into()),
                        appears_transparent: false,
                        traffic_light_position: None,
                    }),
                    focus: false,
                    // Shown once its first page has loaded (`ready-to-show`).
                    show: false,
                    inactive_frame_interval: None,
                    ..Default::default()
                },
                move |window, cx| {
                    crate::covered::keep_painting(window);
                    let events = closed.clone();
                    let slot_on_close = slot.clone();
                    window.on_window_should_close(cx, move |_, _| {
                        // The web view leaves its parent before the window
                        // goes.
                        gone_on_close.set(true);
                        slot_on_close.borrow_mut().take();
                        super::closed();
                        let _ = events.unbounded_send(Event::Closed);
                        true
                    });
                    cx.new(|_| Host {
                        view: slot,
                        placed: Cell::new(None),
                    })
                },
            )
            .map_err(|error| format!("{error}"))?;
        super::opened();
        let parent = handle
            .update(cx, |_, window, _| {
                rwh::HasWindowHandle::window_handle(window).map(|handle| Parent(handle.as_raw()))
            })
            .map_err(|error| format!("{error}"))
            .and_then(|parent| parent.map_err(|error| format!("{error}")));
        #[cfg(target_os = "macos")]
        let parent = parent.and_then(|parent| match parent.0 {
            rwh::RawWindowHandle::AppKit(appkit) => super::macos::content_view(appkit.ns_view)
                .map(|content| {
                    Parent(rwh::RawWindowHandle::AppKit(rwh::AppKitWindowHandle::new(
                        content,
                    )))
                })
                .ok_or_else(|| "The sign-in window has no content view.".to_owned()),
            _ => Ok(parent),
        });
        let parent = match parent {
            Ok(parent) => parent,
            Err(error) => {
                remove(handle, &gone, cx);
                return Err(error);
            }
        };
        let size = handle
            .update(cx, |_, window, _| content_size(window))
            .unwrap_or((options.width, options.height));

        #[cfg(target_os = "windows")]
        let data = match tempfile::Builder::new()
            .prefix("picto-gpui-login-")
            .tempdir()
        {
            Ok(data) => data,
            Err(error) => {
                remove(handle, &gone, cx);
                return Err(error.to_string());
            }
        };
        #[cfg(target_os = "windows")]
        let mut context = wry::WebContext::new(Some(data.path().to_path_buf()));

        let pending = Rc::new(RefCell::new(None::<String>));
        #[cfg(target_os = "macos")]
        let failures = Rc::new(RefCell::new(None));
        let build = cx.spawn({
            let view = view.clone();
            let gone = gone.clone();
            let pending = pending.clone();
            #[cfg(target_os = "macos")]
            let failures = failures.clone();
            async move |cx| {
                if gone.get() {
                    return;
                }
                // Not inside `cx.update`: nothing of the app is borrowed
                // while WebView2 runs the thread's messages.
                #[cfg(target_os = "windows")]
                let builder = {
                    use wry::WebViewBuilderExtWindows as _;
                    super::configure(
                        wry::WebViewBuilder::new_with_web_context(&mut context),
                        &options,
                    )
                    // Electron's windows have no menu of their own on a
                    // right-click; WebView2's (Back, Reload, Inspect…) is
                    // turned off. A page's own menu still shows.
                    .with_default_context_menus(false)
                };
                #[cfg(target_os = "macos")]
                let builder = super::configure(wry::WebViewBuilder::new(), &options);
                let built = builder
                    .with_bounds(bounds(size))
                    .build_as_child(&parent)
                    .map_err(|error| describe(&error));
                match built {
                    // Closed while it was being made.
                    Ok(_) if gone.get() => {}
                    Ok(web_view) => {
                        #[cfg(target_os = "macos")]
                        {
                            *failures.borrow_mut() =
                                super::macos::watch_failures(&web_view, options.events.clone());
                        }
                        #[cfg(target_os = "windows")]
                        super::windows::watch_failures(&web_view, options.events.clone());
                        if let Some(url) = pending.borrow_mut().take() {
                            super::load_into(&web_view, &url);
                        }
                        *view.borrow_mut() = Some(web_view);
                        let _ = handle.update(cx, |host, window, cx| {
                            // The window may have changed size meanwhile.
                            let size = content_size(window);
                            if let Some(view) = host.view.borrow().as_ref() {
                                let _ = view.set_bounds(bounds(size));
                            }
                            host.placed.set(Some(size));
                            cx.notify();
                        });
                    }
                    Err(error) => {
                        let _ = options.events.unbounded_send(Event::Unavailable(error));
                    }
                }
                // WebView2 has made its environment; the context is no
                // longer needed.
                #[cfg(target_os = "windows")]
                drop(context);
            }
        });
        Ok(Self {
            handle,
            view,
            pending,
            gone,
            #[cfg(target_os = "macos")]
            failures,
            #[cfg(target_os = "windows")]
            data: Some(data),
            _build: build,
        })
    }

    pub fn with_view<R>(&self, work: impl FnOnce(&wry::WebView) -> R) -> Option<R> {
        self.view.borrow().as_ref().map(work)
    }

    /// Loads `url`, or keeps it for when the web view is made.
    pub fn load(&self, url: &str) {
        match self.view.borrow().as_ref() {
            Some(view) => super::load_into(view, url),
            None => *self.pending.borrow_mut() = Some(url.to_owned()),
        }
    }

    pub fn set_shown(&self, shown: bool, cx: &mut App) {
        let _ = self.handle.update(cx, |_, window, _| {
            super::show_hosted(window, shown);
        });
    }

    pub fn focus(&self, cx: &mut App) {
        let _ = self.handle.update(cx, |_, window, _| {
            crate::ui::window_chrome::activate(window);
        });
    }

    /// Asks the window to close as its close button does.
    #[cfg(debug_assertions)]
    pub fn close_as_user(&self, cx: &mut App) {
        let _ = self.handle.update(cx, |_, window, _| {
            #[cfg(target_os = "macos")]
            crate::platform::macos::with_ns_window(window, crate::platform::macos::perform_close);
            #[cfg(target_os = "windows")]
            crate::platform::windows::post_close(window);
        });
    }

    /// Saves a picture of the window as it is on the screen.
    #[cfg(debug_assertions)]
    pub fn capture(&self, path: &std::path::Path, cx: &mut App) -> Result<(), String> {
        self.handle
            .update(cx, |_, window, _| {
                #[cfg(target_os = "macos")]
                return super::macos::capture(window, path);
                #[cfg(target_os = "windows")]
                return super::windows::capture(window, path);
            })
            .map_err(|error| error.to_string())?
    }

    pub fn close(self, cx: &mut App) {
        // The web view goes first, so it never outlives the window it sits
        // in.
        self.view.borrow_mut().take();
        #[cfg(target_os = "macos")]
        self.failures.borrow_mut().take();
        remove(self.handle, &self.gone, cx);
        #[cfg(target_os = "windows")]
        if let Some(data) = self.data {
            super::windows::remove_later(data);
        }
    }
}

/// Removes the window, once.
fn remove(handle: WindowHandle<Host>, gone: &Cell<bool>, cx: &mut App) {
    if gone.replace(true) {
        return;
    }
    super::closed();
    let _ = handle.update(cx, |_, window, _| window.remove_window());
}

/// What the user is told when the system has no web view to sign in with.
fn describe(error: &wry::Error) -> String {
    #[cfg(target_os = "windows")]
    if let wry::Error::WebView2Error(_) = error {
        return "Signing in needs the Microsoft Edge WebView2 Runtime, which is not installed."
            .to_owned();
    }
    format!("The sign-in window could not open: {error}")
}
