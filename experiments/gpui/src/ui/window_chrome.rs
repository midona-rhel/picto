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

/// Whether [`system_blur_behind`] blurs what is behind a pop-up window on
/// this system, kept to its panel: on macOS (an `NSVisualEffectView`
/// beneath the panel) and Windows (acrylic, with the window cut to the
/// panel). On Linux GPUI's blur (`ext-background-effect` or KDE's protocol
/// on Wayland; none on X11) covers the whole window, shadow room included,
/// so it is not asked for and the panel draws a more opaque glass instead.
pub const fn system_blurs() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

/// Gives a transparent pop-up window the system's blur of what is behind
/// it, shown only beneath the panel [`place_system_blur`] names. Called as
/// the window opens; until a panel is placed nothing is blurred.
pub fn system_blur_behind(window: &Window) {
    if !system_blurs() {
        return;
    }
    window.set_background_appearance(WindowBackgroundAppearance::Blurred);
    #[cfg(target_os = "macos")]
    appkit::with_ns_window(window, appkit::backdrop::hide);
}

/// Keeps the blur of [`system_blur_behind`] to the panel at `panel` in the
/// window, with corners of `radius`, at `alpha`.
pub fn place_system_blur(window: &Window, panel: Bounds<Pixels>, radius: f32, alpha: f32) {
    #[cfg(target_os = "macos")]
    {
        let height = f32::from(window.viewport_size().height);
        let rect = (
            f32::from(panel.origin.x),
            f32::from(panel.origin.y),
            f32::from(panel.size.width),
            f32::from(panel.size.height),
        );
        appkit::with_ns_window(window, |ns_window| {
            appkit::backdrop::place(ns_window, rect, height, radius, alpha)
        });
    }
    // Acrylic fills the whole window and takes no shape of its own, so the
    // window is cut to the panel's rounded rectangle; the drawn shadow, in
    // the room around the panel, is cut away with it. Its fade is the
    // panel's own opacity.
    #[cfg(target_os = "windows")]
    {
        let _ = alpha;
        win32::cut_to(window, panel, radius);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (window, panel, radius, alpha);
}

/// A panel as a Windows window region: left, top, right and bottom in
/// device pixels (right and bottom one past the panel, where GDI's regions
/// end) and the corners' diameter.
#[cfg(any(test, target_os = "windows"))]
fn panel_region(panel: Bounds<Pixels>, radius: f32, scale: f32) -> [i32; 5] {
    let at = |value: Pixels| (f32::from(value) * scale).round() as i32;
    [
        at(panel.origin.x),
        at(panel.origin.y),
        at(panel.origin.x + panel.size.width) + 1,
        at(panel.origin.y + panel.size.height) + 1,
        (radius * 2. * scale).round() as i32,
    ]
}

/// The window region a pop-up window on Windows is cut to.
#[cfg(target_os = "windows")]
mod win32 {
    use gpui::{Bounds, Pixels, Window};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::ffi::c_void;
    use wgpu::rwh::{HasWindowHandle, RawWindowHandle};

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateRoundRectRgn(
            left: i32,
            top: i32,
            right: i32,
            bottom: i32,
            width: i32,
            height: i32,
        ) -> *mut c_void;
        fn DeleteObject(object: *mut c_void) -> i32;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn SetWindowRgn(window: isize, region: *mut c_void, redraw: i32) -> i32;
    }

    thread_local! {
        /// The region each window was last cut to, so it is set again only
        /// when the panel moves or changes size.
        static CUT: RefCell<HashMap<u64, [i32; 5]>> = RefCell::new(HashMap::new());
    }

    pub(super) fn cut_to(window: &Window, panel: Bounds<Pixels>, radius: f32) {
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::Win32(win32) = handle.as_raw() else {
            return;
        };
        let hwnd = win32.hwnd.get();
        let wanted = super::panel_region(panel, radius, window.scale_factor());
        let id = window.window_handle().window_id().as_u64();
        let unchanged = CUT.with(|cut| cut.borrow_mut().insert(id, wanted) == Some(wanted));
        if unchanged {
            return;
        }
        let [left, top, right, bottom, diameter] = wanted;
        // SAFETY: GDI and user32 calls with the arguments they take; once
        // `SetWindowRgn` succeeds the system owns the region, and it is
        // deleted here only when it was refused.
        unsafe {
            let shape = CreateRoundRectRgn(left, top, right, bottom, diameter, diameter);
            if shape.is_null() {
                return;
            }
            if SetWindowRgn(hwnd, shape, 1) == 0 {
                DeleteObject(shape);
            }
        }
    }
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

    /// The system's blur of what is behind a transparent window, kept to one
    /// rounded panel of it. GPUI's `WindowBackgroundAppearance::Blurred`
    /// lays an `NSVisualEffectView` over the whole content view, beneath
    /// GPUI's own view, with the material's tint and saturation taken out,
    /// so what shows is the plain blur. Here that view is hidden until
    /// [`place_backdrop`] sets it to the panel's rectangle and shape. It
    /// lies inside the content view, so a Core Animation transform of the
    /// content view's layer (`pop_in`) moves it with the panel, and beneath
    /// GPUI's view, so it takes no pointer.
    pub mod backdrop {
        use objc2::msg_send;
        use objc2::rc::Retained;
        use objc2::runtime::{AnyClass, AnyObject, Bool};
        use objc2_foundation::{NSEdgeInsets, NSPoint, NSRect, NSSize};
        use std::ffi::c_void;

        /// GPUI's effect view in the content view of `ns_window`.
        ///
        /// # Safety
        /// `ns_window` is a live NSWindow; called on the main thread.
        unsafe fn effect_view(ns_window: *mut c_void) -> Option<Retained<AnyObject>> {
            let effect = AnyClass::get(c"NSVisualEffectView")?;
            // SAFETY: documented AppKit selectors with these types.
            unsafe {
                let window = ns_window.cast::<AnyObject>();
                let content: Option<Retained<AnyObject>> = msg_send![window, contentView];
                let subviews: Option<Retained<AnyObject>> = msg_send![&*content?, subviews];
                let subviews = subviews?;
                let count: usize = msg_send![&*subviews, count];
                (0..count).find_map(|index| {
                    let view: Option<Retained<AnyObject>> =
                        msg_send![&*subviews, objectAtIndex: index];
                    let view = view?;
                    let is_effect: Bool = msg_send![&*view, isKindOfClass: effect];
                    is_effect.as_bool().then_some(view)
                })
            }
        }

        /// Hides the effect view until a panel places it. Called as the
        /// window opens, before it first shows.
        pub fn hide(ns_window: *mut c_void) {
            // SAFETY: as `effect_view` requires; `setHidden:` and
            // `setAutoresizingMask:` take a BOOL and an NSUInteger.
            unsafe {
                if let Some(view) = effect_view(ns_window) {
                    let _: () = msg_send![&*view, setHidden: true];
                    // It follows the panel, not the window's size.
                    let _: () = msg_send![&*view, setAutoresizingMask: 0usize];
                }
            }
        }

        /// Sets the effect view to the panel at `(x, y, w, h)` (points from
        /// the content's top left) with corners of `radius`, at `alpha`.
        /// `height` is the content's height, as AppKit counts y upwards.
        pub fn place(
            ns_window: *mut c_void,
            (x, y, w, h): (f32, f32, f32, f32),
            height: f32,
            radius: f32,
            alpha: f32,
        ) {
            // SAFETY: as `effect_view` requires; each selector is AppKit's,
            // sent with the argument types it takes.
            unsafe {
                let Some(view) = effect_view(ns_window) else {
                    return;
                };
                let frame = NSRect::new(
                    NSPoint::new(x.into(), (height - y - h).into()),
                    NSSize::new(w.into(), h.into()),
                );
                let _: () = msg_send![&*view, setFrame: frame];
                let _: () = msg_send![&*view, setAlphaValue: f64::from(alpha)];
                let mask: Option<Retained<AnyObject>> = msg_send![&*view, maskImage];
                if mask.is_none()
                    && let Some(mask) = rounded_mask(radius.into())
                {
                    let _: () = msg_send![&*view, setMaskImage: &*mask];
                }
                let _: () = msg_send![&*view, setHidden: false];
            }
        }

        /// A rounded rectangle of `radius` that stretches to any size: its
        /// corners are kept and its middle drawn out (`capInsets`), as
        /// `NSVisualEffectView.maskImage` documents for rounded shapes.
        fn rounded_mask(radius: f64) -> Option<Retained<AnyObject>> {
            let (image, path, color) = (
                AnyClass::get(c"NSImage")?,
                AnyClass::get(c"NSBezierPath")?,
                AnyClass::get(c"NSColor")?,
            );
            let side = radius * 2. + 1.;
            let draw = block2::RcBlock::new(move |rect: NSRect| -> Bool {
                // SAFETY: AppKit's drawing selectors, called by AppKit on
                // the main thread while the image draws.
                unsafe {
                    let shape: Option<Retained<AnyObject>> = msg_send![
                        path,
                        bezierPathWithRoundedRect: rect,
                        xRadius: radius,
                        yRadius: radius
                    ];
                    let black: Option<Retained<AnyObject>> = msg_send![color, blackColor];
                    if let (Some(shape), Some(black)) = (shape, black) {
                        let _: () = msg_send![&*black, set];
                        let _: () = msg_send![&*shape, fill];
                    }
                }
                Bool::YES
            });
            // SAFETY: `imageWithSize:flipped:drawingHandler:` takes an
            // NSSize, a BOOL and a block of this signature; `setCapInsets:`
            // an NSEdgeInsets; `setResizingMode:` an NSInteger (stretch, 1).
            unsafe {
                let mask: Option<Retained<AnyObject>> = msg_send![
                    image,
                    imageWithSize: NSSize::new(side, side),
                    flipped: false,
                    drawingHandler: &*draw
                ];
                let mask = mask?;
                let insets = NSEdgeInsets {
                    top: radius,
                    left: radius,
                    bottom: radius,
                    right: radius,
                };
                let _: () = msg_send![&*mask, setCapInsets: insets];
                let _: () = msg_send![&*mask, setResizingMode: 1isize];
                Some(mask)
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

#[cfg(test)]
mod tests {
    use super::panel_region;
    use gpui::{Bounds, point, px, size};

    #[test]
    fn a_panel_is_cut_out_in_device_pixels_with_its_round_corners() {
        let panel = Bounds::new(point(px(40.), px(40.5)), size(px(270.), px(300.)));
        assert_eq!(panel_region(panel, 12., 1.), [40, 41, 311, 342, 24]);
        assert_eq!(panel_region(panel, 12., 1.5), [60, 61, 466, 512, 36]);
    }
}
