//! What every Picto window draws around its content: the macOS rim, and the
//! close button of a window that hides the system's.
use crate::{motion, theme};
use gpui::*;

/// `.shellMac::after`: the window's rim, over everything. Top 25% white,
/// sides 15%, bottom 10%; one border has one colour, so the bottom's goes
/// all the way round and the others are laid over it.
pub fn rim() -> impl IntoElement {
    let over = |alpha: f32| Rgba {
        r: 1.,
        g: 1.,
        b: 1.,
        a: alpha,
    };
    let edge = || div().absolute().inset_0().rounded(px(theme::WINDOW_RADIUS));
    deferred(
        div()
            .absolute()
            .inset_0()
            .child(edge().border_1().border_color(over(0.10)))
            // 1 − 0.9 × (1 − a) gives 0.15 at the sides, 0.25 on top.
            .child(edge().border_l_1().border_r_1().border_color(over(0.0556)))
            .child(edge().border_t_1().border_color(over(0.1667))),
    )
    .with_priority(100)
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

/// The few Objective-C runtime calls the app makes to AppKit for what GPUI
/// has no option for.
#[cfg(target_os = "macos")]
pub(crate) mod appkit {
    use gpui::Window;
    use std::ffi::{CStr, c_char, c_void};
    use wgpu::rwh::{HasWindowHandle, RawWindowHandle};

    #[link(name = "objc", kind = "dylib")]
    unsafe extern "C" {
        fn sel_registerName(name: *const c_char) -> *const c_void;
        fn object_getClass(object: *mut c_void) -> *mut c_void;
        fn class_replaceMethod(
            class: *mut c_void,
            selector: *const c_void,
            implementation: *const c_void,
            types: *const c_char,
        ) -> *const c_void;
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

    /// Gives every object of `object`'s class `implementation` for
    /// `selector`, with the Objective-C type encoding `types`.
    ///
    /// # Safety
    /// `object` is a live Objective-C object, and `implementation` is an
    /// `extern "C"` function matching `types` and the method's signature.
    pub unsafe fn replace_method(
        object: *mut c_void,
        selector: *const c_void,
        implementation: *const c_void,
        types: &CStr,
    ) {
        // SAFETY: as the caller promises.
        unsafe {
            class_replaceMethod(
                object_getClass(object),
                selector,
                implementation,
                types.as_ptr(),
            );
        }
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

struct Press {
    hover: motion::Fade,
    press: motion::Fade,
}

/// `WindowCloseButton`: a 28px square with a thin 16px cross, over the
/// subtle overlay under the pointer and the pressed overlay while held,
/// changing over 150 ms.
pub fn close_button(
    id: impl Into<ElementId>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let theme = theme::theme(cx);
    let id = id.into();
    let state = window.use_keyed_state(id.clone(), cx, |_, _| Press {
        hover: motion::Fade::default(),
        press: motion::Fade::default(),
    });
    let (hover, press) = {
        let state = state.read(cx);
        if state.hover.animating() || state.press.animating() {
            window.request_animation_frame();
        }
        (state.hover.value(), state.press.value())
    };
    // `--color-subtle-overlay` under the pointer, `--color-pressed-overlay`
    // while held.
    let hovered = 10. * hover;
    let overlay = hovered + (20. - hovered) * press;
    let (entered, down, up, out) = (state.clone(), state.clone(), state.clone(), state);
    let release = |state: &Entity<Press>, cx: &mut App| {
        state.update(cx, |state, cx| {
            state.press.set(false, motion::BUTTON_HOVER);
            cx.notify();
        })
    };
    div()
        .id(id)
        .occlude()
        .size(px(theme::CONTROL_MD))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_CONTROL))
        .bg(theme.contrast(overlay))
        .cursor_pointer()
        .child(super::icon("window-close", 16., theme.text()))
        .on_hover(move |hovered, _, cx| {
            entered.update(cx, |state, cx| {
                state.hover.set(*hovered, motion::BUTTON_HOVER);
                cx.notify();
            })
        })
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            down.update(cx, |state, cx| {
                state.press.set(true, motion::BUTTON_HOVER);
                cx.notify();
            })
        })
        .on_mouse_up(MouseButton::Left, move |_, _, cx| release(&up, cx))
        .on_mouse_up_out(MouseButton::Left, move |_, _, cx| release(&out, cx))
        .on_click(on_click)
}
