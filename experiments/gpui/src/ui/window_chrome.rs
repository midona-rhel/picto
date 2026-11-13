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

/// Takes the window off the screen without closing it (`orderOut:`), as a
/// Mac app's main window goes when it is closed while the app runs on.
pub fn order_out(window: &Window) {
    #[cfg(target_os = "macos")]
    appkit::with_ns_window(window, |ns_window| {
        // SAFETY: `orderOut:` takes an object (the sender, nil here),
        // called on the main thread.
        unsafe {
            let send_object: extern "C" fn(
                *mut std::ffi::c_void,
                *const std::ffi::c_void,
                *mut std::ffi::c_void,
            ) = std::mem::transmute(appkit::send());
            send_object(
                ns_window,
                appkit::selector(c"orderOut:"),
                std::ptr::null_mut(),
            );
        }
    });
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

/// The Window menu's Bring All to Front (`arrangeInFront:`).
pub fn bring_all_to_front(cx: &mut App) {
    #[cfg(target_os = "macos")]
    {
        let _ = cx;
        appkit::arrange_in_front();
    }
    #[cfg(not(target_os = "macos"))]
    for window in cx.windows() {
        let _ = window.update(cx, |_, window, _| window.activate_window());
    }
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
        fn objc_getClass(name: *const c_char) -> *mut c_void;
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

    /// Turns off the animation AppKit plays as a window appears (a panel
    /// zooms in), for windows that bring their own.
    pub fn without_appear_animation(ns_window: *mut c_void) {
        /// `NSWindowAnimationBehaviorNone`.
        const NONE: isize = 2;
        // SAFETY: `ns_window` is a live NSWindow; `setAnimationBehavior:`
        // takes an `NSInteger`.
        unsafe {
            let send_integer: extern "C" fn(*mut c_void, *const c_void, isize) =
                std::mem::transmute(send());
            send_integer(ns_window, selector(c"setAnimationBehavior:"), NONE);
        }
    }

    /// Plays CSS's `transform: scale(from) → scale(1)` on everything the
    /// window draws, about the point `fixed` (in points from the content's
    /// centre, x to the right and y up), over `seconds` on the cubic bezier
    /// `curve`. Core Animation scales the window's layer, so the contents
    /// scale with their panel, which GPUI cannot do for an element.
    pub fn pop_in(
        ns_window: *mut c_void,
        fixed: (f32, f32),
        from: f32,
        seconds: f64,
        curve: (f32, f32, f32, f32),
    ) {
        type Id = *mut c_void;
        type Sel = *const c_void;
        // SAFETY: every receiver is checked for null before it is sent to;
        // each cast gives `objc_msgSend` the signature of the AppKit or
        // QuartzCore method named beside it. Called on the main thread.
        unsafe {
            let send_id: extern "C" fn(Id, Sel) -> Id = std::mem::transmute(send());
            let send_object: extern "C" fn(Id, Sel, Id) -> Id = std::mem::transmute(send());
            let send_two: extern "C" fn(Id, Sel, Id, Id) = std::mem::transmute(send());
            let send_text: extern "C" fn(Id, Sel, *const c_char) -> Id =
                std::mem::transmute(send());
            let send_double: extern "C" fn(Id, Sel, f64) -> Id = std::mem::transmute(send());
            let send_curve: extern "C" fn(Id, Sel, f32, f32, f32, f32) -> Id =
                std::mem::transmute(send());
            let view = send_id(ns_window, selector(c"contentView"));
            if view.is_null() {
                return;
            }
            let layer = send_id(view, selector(c"layer"));

            let (animation_class, timing_class, number_class, string_class) = (
                objc_getClass(c"CABasicAnimation".as_ptr()),
                objc_getClass(c"CAMediaTimingFunction".as_ptr()),
                objc_getClass(c"NSNumber".as_ptr()),
                objc_getClass(c"NSString".as_ptr()),
            );
            if layer.is_null()
                || animation_class.is_null()
                || timing_class.is_null()
                || number_class.is_null()
                || string_class.is_null()
            {
                return;
            }
            let text = |text: &CStr| {
                send_text(
                    string_class,
                    selector(c"stringWithUTF8String:"),
                    text.as_ptr(),
                )
            };
            let number = |value: f32| {
                send_double(number_class, selector(c"numberWithDouble:"), value.into())
            };
            let timing = send_curve(
                timing_class,
                selector(c"functionWithControlPoints::::"),
                curve.0,
                curve.1,
                curve.2,
                curve.3,
            );
            // A scale about a point other than the centre is the scale about
            // the centre and a shift of (1 - scale) × that point; the three
            // run together on one curve, so the point stays where it is.
            for (path, start, end) in [
                (c"transform.scale", from, 1.),
                (c"transform.translation.x", (1. - from) * fixed.0, 0.),
                (c"transform.translation.y", (1. - from) * fixed.1, 0.),
            ] {
                let animation = send_object(
                    animation_class,
                    selector(c"animationWithKeyPath:"),
                    text(path),
                );
                if animation.is_null() {
                    return;
                }
                send_object(animation, selector(c"setFromValue:"), number(start));
                send_object(animation, selector(c"setToValue:"), number(end));
                send_double(animation, selector(c"setDuration:"), seconds);
                send_object(animation, selector(c"setTimingFunction:"), timing);
                send_two(
                    layer,
                    selector(c"addAnimation:forKey:"),
                    animation,
                    text(path),
                );
            }
        }
    }

    /// Runs `work` with the window's NSWindow, if it has one.
    /// `[NSApp arrangeInFront:nil]`: the app's windows in front of other
    /// apps', in their order.
    pub fn arrange_in_front() {
        // SAFETY: `sharedApplication` is a class method returning the app,
        // and `arrangeInFront:` takes an object (the sender, nil here);
        // called on the main thread.
        unsafe {
            let class = objc_getClass(c"NSApplication".as_ptr());
            if class.is_null() {
                return;
            }
            let send_id: extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
                std::mem::transmute(send());
            let app = send_id(class, selector(c"sharedApplication"));
            if app.is_null() {
                return;
            }
            let send_object: extern "C" fn(*mut c_void, *const c_void, *mut c_void) =
                std::mem::transmute(send());
            send_object(app, selector(c"arrangeInFront:"), std::ptr::null_mut());
        }
    }

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
