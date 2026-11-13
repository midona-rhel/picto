//! What every Picto window draws around its content: the macOS rim, and the
//! close button of a window that hides the system's.
use crate::theme;
use gpui::*;

/// `.shellMac::after`: the window's rim, over everything, in the glass
/// edges.
pub fn rim(cx: &App) -> impl IntoElement {
    deferred(super::glass_edges(theme::WINDOW_RADIUS, cx)).with_priority(100)
}

/// Hides the traffic lights of a window that draws its own close button.
/// GPUI has no option for this, so it asks AppKit directly.
pub fn hide_window_buttons(window: &Window) {
    #[cfg(target_os = "macos")]
    appkit::with_ns_window(window, |ns_window| {
        // SAFETY: `standardWindowButton:` and `setHidden:` are NSWindow and
        // NSView methods with these signatures, called on the main thread.
        unsafe {
            let send_button: extern "C" fn(
                *mut std::ffi::c_void,
                *const std::ffi::c_void,
                usize,
            ) -> *mut std::ffi::c_void = std::mem::transmute(appkit::send());
            let send_bool: extern "C" fn(*mut std::ffi::c_void, *const std::ffi::c_void, bool) =
                std::mem::transmute(appkit::send());
            // Close, miniaturize and zoom.
            for kind in 0..3 {
                let button =
                    send_button(ns_window, appkit::selector(c"standardWindowButton:"), kind);
                if !button.is_null() {
                    send_bool(button, appkit::selector(c"setHidden:"), true);
                }
            }
        }
    });
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

/// Keeps the window's content at `width / height` while it is resized, as
/// Electron's `setAspectRatio` does.
pub fn lock_aspect_ratio(window: &Window, ratio: f64) {
    #[cfg(target_os = "macos")]
    appkit::with_ns_window(window, |ns_window| {
        #[repr(C)]
        struct NsSize {
            width: f64,
            height: f64,
        }
        // SAFETY: `setContentAspectRatio:` takes an NSSize (two doubles),
        // called on the main thread.
        unsafe {
            let send_size: extern "C" fn(*mut std::ffi::c_void, *const std::ffi::c_void, NsSize) =
                std::mem::transmute(appkit::send());
            send_size(
                ns_window,
                appkit::selector(c"setContentAspectRatio:"),
                NsSize {
                    width: ratio,
                    height: 1.,
                },
            );
        }
    });
    #[cfg(not(target_os = "macos"))]
    let _ = (window, ratio);
}

/// Floats the window above others, or puts it back among them, as
/// Electron's `setAlwaysOnTop` does with its default `floating` level.
pub fn set_always_on_top(window: &Window, on_top: bool) {
    #[cfg(target_os = "macos")]
    appkit::with_ns_window(window, |ns_window| {
        // `NSFloatingWindowLevel` and `NSNormalWindowLevel`.
        let level: isize = if on_top { 3 } else { 0 };
        // SAFETY: `setLevel:` takes an NSInteger, called on the main thread.
        unsafe {
            let send_level: extern "C" fn(*mut std::ffi::c_void, *const std::ffi::c_void, isize) =
                std::mem::transmute(appkit::send());
            send_level(ns_window, appkit::selector(c"setLevel:"), level);
        }
    });
    #[cfg(not(target_os = "macos"))]
    let _ = (window, on_top);
}

#[cfg(target_os = "macos")]
mod appkit {
    use gpui::Window;
    use std::ffi::{CStr, c_char, c_void};
    use wgpu::rwh::{HasWindowHandle, RawWindowHandle};

    #[link(name = "objc", kind = "dylib")]
    unsafe extern "C" {
        fn sel_registerName(name: *const c_char) -> *const c_void;
        fn objc_msgSend();
    }

    pub fn selector(name: &CStr) -> *const c_void {
        // SAFETY: registering a selector name has no preconditions.
        unsafe { sel_registerName(name.as_ptr()) }
    }

    /// `objc_msgSend`, to be cast to the signature of the method sent.
    pub fn send() -> unsafe extern "C" fn() {
        objc_msgSend
    }

    /// Runs `work` with the window's NSWindow, if it has one.
    pub fn with_ns_window(window: &Window, work: impl FnOnce(*mut c_void)) {
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return;
        };
        // SAFETY: `ns_view` is the live content view of this window, and
        // `window` is an NSView method returning its NSWindow.
        let ns_window = unsafe {
            let send_id: extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
                std::mem::transmute(send());
            send_id(appkit.ns_view.as_ptr(), selector(c"window"))
        };
        if !ns_window.is_null() {
            work(ns_window);
        }
    }
}

/// `WindowCloseButton`: a 28px square with a thin 16px cross, as the shared
/// icon button's window-close look.
pub fn close_button(
    id: impl Into<ElementId>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> super::icon_button::IconButton {
    super::icon_button(id, "window-close")
        .look(super::icon_button::Look::WindowClose)
        .on_click(on_click)
}
