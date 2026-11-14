//! macOS and Windows: the web view as a child of a GPUI window of its own,
//! with the system's title bar ("Login: <service>"), white beneath the page
//! as production's window is, filling the window as it is resized.
//!
//! The web view is made outside any GPUI update, from the window's handle:
//! WebView2 waits for its environment by running the thread's messages,
//! which must not re-enter GPUI while the app is borrowed.
use super::{Event, Options};
use gpui::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wry::raw_window_handle as rwh;

type Slot = Rc<RefCell<Option<wry::WebView>>>;

pub struct Hosted {
    handle: WindowHandle<Host>,
    view: Slot,
    /// WebView2's user data, a fresh folder per window, removed with it.
    #[cfg(target_os = "windows")]
    data: Option<tempfile::TempDir>,
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
        let viewport = window.viewport_size();
        let zoom = window.ui_zoom();
        let size = (
            f32::from(viewport.width) * zoom,
            f32::from(viewport.height) * zoom,
        );
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
        // SAFETY: the handle is of a window that stays open while the web
        // view is built from it, on the same thread.
        Ok(unsafe { rwh::WindowHandle::borrow_raw(self.0) })
    }
}

impl Hosted {
    pub fn open(options: Options, cx: &mut App) -> Result<Self, String> {
        let view: Slot = Rc::new(RefCell::new(None));
        let mut bounds_ = Bounds::centered(None, size(px(options.width), px(options.height)), cx);
        bounds_.origin = bounds_.origin.map(|at| at.floor());
        let closed = options.events.clone();
        let slot = view.clone();
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
                        slot_on_close.borrow_mut().take();
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
        let parent = handle
            .update(cx, |_, window, _| {
                rwh::HasWindowHandle::window_handle(window).map(|handle| Parent(handle.as_raw()))
            })
            .map_err(|error| format!("{error}"))?
            .map_err(|error| format!("{error}"))?;
        #[cfg(target_os = "macos")]
        let parent = match parent.0 {
            rwh::RawWindowHandle::AppKit(appkit) => {
                let content = super::macos::content_view(appkit.ns_view)
                    .ok_or_else(|| "The sign-in window has no content view.".to_owned())?;
                Parent(rwh::RawWindowHandle::AppKit(rwh::AppKitWindowHandle::new(
                    content,
                )))
            }
            _ => parent,
        };
        let size = handle
            .update(cx, |_, window, _| {
                let viewport = window.viewport_size();
                let zoom = window.ui_zoom();
                (
                    f32::from(viewport.width) * zoom,
                    f32::from(viewport.height) * zoom,
                )
            })
            .unwrap_or((options.width, options.height));

        #[cfg(target_os = "windows")]
        let data = tempfile::Builder::new()
            .prefix("picto-gpui-login-")
            .tempdir()
            .map_err(|error| error.to_string())?;
        #[cfg(target_os = "windows")]
        let mut context = wry::WebContext::new(Some(data.path().to_path_buf()));
        #[cfg(target_os = "windows")]
        let builder = {
            use wry::WebViewBuilderExtWindows as _;
            super::configure(
                wry::WebViewBuilder::new_with_web_context(&mut context),
                &options,
            )
            // Production's login windows have no Edge menus or keys of
            // their own beyond the page's.
            .with_default_context_menus(true)
        };
        #[cfg(target_os = "macos")]
        let builder = super::configure(wry::WebViewBuilder::new(), &options);
        let built = builder
            .with_bounds(bounds(size))
            .build_as_child(&parent)
            .map_err(|error| describe(&error));
        match built {
            Ok(web_view) => {
                *view.borrow_mut() = Some(web_view);
                let _ = handle.update(cx, |host, _, cx| {
                    host.placed.set(Some(size));
                    cx.notify();
                });
                Ok(Self {
                    handle,
                    view,
                    #[cfg(target_os = "windows")]
                    data: Some(data),
                })
            }
            Err(error) => {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
                Err(error)
            }
        }
    }

    pub fn with_view<R>(&self, work: impl FnOnce(&wry::WebView) -> R) -> Option<R> {
        self.view.borrow().as_ref().map(work)
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

    pub fn close(self, cx: &mut App) {
        // The web view goes first, so it never outlives the window it sits
        // in.
        self.view.borrow_mut().take();
        let _ = self
            .handle
            .update(cx, |_, window, _| window.remove_window());
        #[cfg(target_os = "windows")]
        if let Some(data) = self.data {
            super::windows::remove_later(data);
        }
    }
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
