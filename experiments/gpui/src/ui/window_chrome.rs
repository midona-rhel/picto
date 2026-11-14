//! What every Picto window draws around its content: the macOS rim, the
//! close button of a window that hides the system's, and on Windows and
//! Linux, where production's windows are frameless, the minimise, maximise
//! and close buttons and (Linux) the edges that resize the window.
use crate::theme;
use gpui::*;

/// The frame every Picto window asks the system for: no system titlebar
/// (the app draws its own; on macOS the traffic lights stay, at
/// `traffic_lights` if given), with the app deciding where the window is
/// dragged from; and on Linux the client's decorations, as production's
/// windows are frameless there (`frame: false`).
pub fn frameless(
    title: impl Into<SharedString>,
    traffic_lights: Option<Point<Pixels>>,
) -> WindowOptions {
    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            appears_transparent: true,
            traffic_light_position: traffic_lights,
        }),
        app_owns_titlebar_drag: true,
        window_decorations: DECORATIONS,
        ..Default::default()
    }
}

/// The decorations [`frameless`] asks for: the client's on Linux, the
/// system's default elsewhere.
pub const DECORATIONS: Option<WindowDecorations> =
    if cfg!(any(target_os = "linux", target_os = "freebsd")) {
        Some(WindowDecorations::Client)
    } else {
        None
    };

/// How far a titlebar's left block keeps its controls from the window's
/// left edge: clear of the traffic lights on macOS (`.titlebarLeftMac`),
/// its 8px padding elsewhere (`.titlebarLeft`).
pub const TITLEBAR_INSET: f32 = if cfg!(target_os = "macos") { 74. } else { 8. };

/// `.shellMac::after`: the window's rim, over everything, in the glass
/// edges. Production draws it on macOS only.
pub fn rim(cx: &App) -> Option<impl IntoElement + use<>> {
    cfg!(target_os = "macos")
        .then(|| deferred(super::glass_edges(theme::WINDOW_RADIUS, cx)).with_priority(100))
}

/// Whether the app draws the window's minimise, maximise and close buttons
/// (`WindowControls`, shown on Windows and Linux): always on Windows, whose
/// system titlebar these windows hide; on Linux while the window's
/// decorations are the client's (an X11 desktop without a compositor keeps
/// the window manager's frame, buttons and all). Never on macOS.
pub fn draws_caption_buttons(window: &Window) -> bool {
    if cfg!(target_os = "macos") {
        false
    } else if cfg!(target_os = "windows") {
        true
    } else {
        matches!(window.window_decorations(), Decorations::Client { .. })
    }
}

/// `WindowControls`: minimise, maximise and close, 28px each with no gap,
/// for the main window's titlebar. On Windows they are the system's caption
/// buttons (`WindowControlArea`), so the system performs them, with its
/// snap layouts over maximise; on Linux they ask the window manager.
pub fn caption_buttons(id: &str, window: &Window) -> Option<Div> {
    if !draws_caption_buttons(window) {
        return None;
    }
    let button = |part: &str, glyph: &'static str, glyph_size: f32, area: WindowControlArea| {
        let button = super::icon_button(SharedString::from(format!("{id}-{part}")), glyph)
            .look(if area == WindowControlArea::Close {
                super::icon_button::Look::CaptionClose
            } else {
                super::icon_button::Look::WindowClose
            })
            .glyph(glyph_size)
            .tip(super::tip(match area {
                WindowControlArea::Min => "Minimize",
                WindowControlArea::Max => "Maximize",
                _ => "Close",
            }))
            .window_control(area);
        if cfg!(target_os = "windows") {
            button
        } else {
            button.on_click(move |_, window, _| match area {
                WindowControlArea::Min => window.minimize_window(),
                WindowControlArea::Max => window.zoom_window(),
                _ => window.remove_window(),
            })
        }
    };
    Some(
        div()
            .flex()
            .flex_none()
            .items_center()
            .child(button(
                "minimize",
                "window-minimize",
                16.,
                WindowControlArea::Min,
            ))
            .child(button(
                "maximize",
                "window-maximize",
                12.,
                WindowControlArea::Max,
            ))
            .child(button(
                "close",
                "window-close",
                16.,
                WindowControlArea::Close,
            )),
    )
}

/// How far into the window its edges resize it, and how far along an edge
/// a corner reaches.
const RESIZE_EDGE: f32 = 5.;
const RESIZE_CORNER: f32 = 10.;

/// The strips along a window of `size` that resize it, and which edge or
/// corner each moves. A side tiled against the screen's edge (or every
/// side of a maximised window) has none.
fn resize_zones(size: Size<Pixels>, tiling: Tiling) -> Vec<(ResizeEdge, Bounds<Pixels>)> {
    let (width, height) = (f32::from(size.width), f32::from(size.height));
    let (edge, corner) = (RESIZE_EDGE, RESIZE_CORNER);
    let rect = |x: f32, y: f32, w: f32, h: f32| Bounds::new(point(px(x), px(y)), size_of(w, h));
    fn size_of(w: f32, h: f32) -> Size<Pixels> {
        gpui::size(px(w.max(0.)), px(h.max(0.)))
    }
    let mut zones = Vec::new();
    if !tiling.top {
        zones.push((ResizeEdge::Top, rect(corner, 0., width - 2. * corner, edge)));
    }
    if !tiling.bottom {
        zones.push((
            ResizeEdge::Bottom,
            rect(corner, height - edge, width - 2. * corner, edge),
        ));
    }
    if !tiling.left {
        zones.push((
            ResizeEdge::Left,
            rect(0., corner, edge, height - 2. * corner),
        ));
    }
    if !tiling.right {
        zones.push((
            ResizeEdge::Right,
            rect(width - edge, corner, edge, height - 2. * corner),
        ));
    }
    let corners = [
        (ResizeEdge::TopLeft, tiling.top || tiling.left, 0., 0.),
        (
            ResizeEdge::TopRight,
            tiling.top || tiling.right,
            width - corner,
            0.,
        ),
        (
            ResizeEdge::BottomLeft,
            tiling.bottom || tiling.left,
            0.,
            height - corner,
        ),
        (
            ResizeEdge::BottomRight,
            tiling.bottom || tiling.right,
            width - corner,
            height - corner,
        ),
    ];
    for (which, tiled, x, y) in corners {
        if !tiled {
            zones.push((which, rect(x, y, corner, corner)));
        }
    }
    zones
}

/// The pointer over a resizing edge or corner.
fn resize_cursor(edge: ResizeEdge) -> CursorStyle {
    match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
        ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
    }
}

/// The edges that resize a window whose decorations are the client's (a
/// frameless window on Linux): a press on one hands the resize to the
/// window manager. Drawn last, over the window's content, by each window's
/// root; nothing while the system draws the frame.
pub fn resize_edges(window: &Window) -> Option<impl IntoElement + use<>> {
    let Decorations::Client { tiling } = window.window_decorations() else {
        return None;
    };
    if !window.is_resizable() {
        return None;
    }
    let zones = resize_zones(window.viewport_size(), tiling);
    Some(
        div()
            .absolute()
            .inset_0()
            .children(zones.into_iter().enumerate().map(|(index, (edge, at))| {
                div()
                    .id(("resize-edge", index))
                    .absolute()
                    .left(at.origin.x)
                    .top(at.origin.y)
                    .w(at.size.width)
                    .h(at.size.height)
                    .occlude()
                    .cursor(resize_cursor(edge))
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        window.start_window_resize(edge);
                    })
            })),
    )
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

/// Whether [`system_blur_behind`] blurs what is behind a pop-up window:
/// on macOS, the one system whose menus are pop-up windows
/// (`ui::menu::presentation`), through an `NSVisualEffectView` beneath the
/// panel. Windows and Linux show menus inside the window, as production's
/// renderer does, where the window's own backdrop blur (or, on Windows,
/// the more opaque glass) applies.
pub const fn system_blurs() -> bool {
    cfg!(target_os = "macos")
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
    #[cfg(not(target_os = "macos"))]
    let _ = (window, panel, radius, alpha);
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
    use super::{RESIZE_CORNER, RESIZE_EDGE, resize_zones};
    use gpui::{Bounds, ResizeEdge, Tiling, point, px, size};

    #[test]
    fn a_floating_window_resizes_from_every_edge_and_corner() {
        let zones = resize_zones(size(px(800.), px(600.)), Tiling::default());
        assert_eq!(zones.len(), 8);
        let at = |edge| zones.iter().find(|(which, _)| *which == edge).unwrap().1;
        assert_eq!(
            at(ResizeEdge::Right),
            Bounds::new(
                point(px(800. - RESIZE_EDGE), px(RESIZE_CORNER)),
                size(px(RESIZE_EDGE), px(600. - 2. * RESIZE_CORNER))
            )
        );
        assert_eq!(
            at(ResizeEdge::BottomLeft).origin,
            point(px(0.), px(600. - RESIZE_CORNER))
        );
    }

    #[test]
    fn tiled_sides_and_their_corners_do_not_resize() {
        let left_half = Tiling {
            top: true,
            bottom: true,
            left: true,
            right: false,
        };
        let edges: Vec<ResizeEdge> = resize_zones(size(px(800.), px(600.)), left_half)
            .into_iter()
            .map(|(edge, _)| edge)
            .collect();
        assert_eq!(edges, [ResizeEdge::Right]);
        assert!(resize_zones(size(px(800.), px(600.)), Tiling::tiled()).is_empty());
    }
}
