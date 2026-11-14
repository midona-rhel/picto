//! The AppKit and Core Animation calls the app makes for what GPUI has no
//! option for, through `objc2`, whose `msg_send!` checks each message's
//! argument and return types against the method's own in debug builds.
//!
//! Every function takes the window's `NSWindow` from [`with_ns_window`]
//! and runs on the main thread, where GPUI calls into the app; AppKit
//! requires both.
use gpui::Window;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, MessageReceiver, Sel};
use objc2::sel;
use objc2_foundation::{NSNumber, NSSize, NSString};
use wgpu::rwh::{HasWindowHandle, RawWindowHandle};

/// Runs `work` with the window's `NSWindow`, if it has one.
pub fn with_ns_window(window: &Window, work: impl FnOnce(&AnyObject)) {
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return;
    };
    // SAFETY: `ns_view` is the live content view GPUI made for this window
    // (the handle borrows the window, which owns it), and `window` is an
    // NSView method that takes nothing and returns its NSWindow or nil.
    let ns_window: Option<Retained<AnyObject>> = unsafe {
        let view = appkit.ns_view.cast::<AnyObject>().as_ref();
        msg_send![view, window]
    };
    if let Some(ns_window) = ns_window {
        work(&ns_window);
    }
}

/// Hides the close, minimise and zoom buttons (the traffic lights).
pub fn hide_window_buttons(ns_window: &AnyObject) {
    // `NSWindowCloseButton`, `NSWindowMiniaturizeButton`,
    // `NSWindowZoomButton`.
    for kind in 0usize..3 {
        // SAFETY: `standardWindowButton:` is an NSWindow method taking an
        // `NSWindowButton` (an NSUInteger) and returning the button or nil;
        // `setHidden:` an NSView method taking a BOOL.
        unsafe {
            let button: Option<Retained<AnyObject>> =
                msg_send![ns_window, standardWindowButton: kind];
            if let Some(button) = button {
                let _: () = msg_send![&*button, setHidden: true];
            }
        }
    }
}

/// Keeps the window's content at `ratio` (width over height) while it is
/// resized.
pub fn lock_aspect_ratio(ns_window: &AnyObject, ratio: f64) {
    // SAFETY: `setContentAspectRatio:` is an NSWindow method taking an
    // NSSize.
    unsafe {
        let _: () = msg_send![ns_window, setContentAspectRatio: NSSize::new(ratio, 1.)];
    }
}

/// Floats the window over others (`NSFloatingWindowLevel`) or puts it back
/// among them (`NSNormalWindowLevel`).
pub fn set_floating(ns_window: &AnyObject, floating: bool) {
    let level: isize = if floating { 3 } else { 0 };
    // SAFETY: `setLevel:` is an NSWindow method taking an `NSWindowLevel`
    // (an NSInteger).
    unsafe {
        let _: () = msg_send![ns_window, setLevel: level];
    }
}

/// Takes the window off the screen without closing it (`orderOut:`).
pub fn order_out(ns_window: &AnyObject) {
    // SAFETY: `orderOut:` is an NSWindow method taking its sender, an
    // object that may be nil.
    unsafe {
        let _: () = msg_send![ns_window, orderOut: None::<&AnyObject>];
    }
}

/// Turns off the shadow and edge highlight the system traces round what a
/// window draws.
pub fn without_shadow(ns_window: &AnyObject) {
    // SAFETY: `setHasShadow:` is an NSWindow method taking a BOOL.
    unsafe {
        let _: () = msg_send![ns_window, setHasShadow: false];
    }
}

/// Turns off the animation AppKit plays as a window appears (a panel zooms
/// in), for windows that bring their own.
pub fn without_appear_animation(ns_window: &AnyObject) {
    /// `NSWindowAnimationBehaviorNone`.
    const NONE: isize = 2;
    // SAFETY: `setAnimationBehavior:` is an NSWindow method taking an
    // `NSWindowAnimationBehavior` (an NSInteger).
    unsafe {
        let _: () = msg_send![ns_window, setAnimationBehavior: NONE];
    }
}

/// `[NSApp arrangeInFront:nil]`: the app's windows in front of other apps',
/// in their order.
pub fn arrange_in_front() {
    let Some(class) = AnyClass::get(c"NSApplication") else {
        return;
    };
    // SAFETY: `sharedApplication` is an NSApplication class method taking
    // nothing and returning the app; `arrangeInFront:` takes its sender, an
    // object that may be nil.
    unsafe {
        let app: Option<Retained<AnyObject>> = msg_send![class, sharedApplication];
        if let Some(app) = app {
            let _: () = msg_send![&*app, arrangeInFront: None::<&AnyObject>];
        }
    }
}

/// Has the window report itself always fully visible, so that GPUI, which
/// stops a window's frames once AppKit reports it covered, keeps drawing it
/// behind other applications.
pub fn keep_painting_while_covered(ns_window: &AnyObject) {
    /// `NSWindowOcclusionStateVisible`, whatever covers the window.
    extern "C-unwind" fn visible(_: &AnyObject, _: Sel) -> usize {
        1 << 1
    }
    let implementation: extern "C-unwind" fn(&AnyObject, Sel) -> usize = visible;
    // SAFETY: the class is the window's own (GPUI's window class, which is
    // also its delegate); `occlusionState` takes nothing and returns an
    // NSUInteger, which `visible` does, and "Q@:" encodes that signature.
    // `windowDidChangeOcclusionState:` is the delegate method GPUI's class
    // implements, taking a notification (nil here) and returning nothing.
    unsafe {
        let class = ns_window.class() as *const AnyClass as *mut AnyClass;
        objc2::ffi::class_replaceMethod(
            class,
            sel!(occlusionState),
            std::mem::transmute::<
                extern "C-unwind" fn(&AnyObject, Sel) -> usize,
                objc2::runtime::Imp,
            >(implementation),
            c"Q@:".as_ptr(),
        );
        // Have the window look at its (new) state and start its frames.
        let _: () = msg_send![ns_window, windowDidChangeOcclusionState: None::<&AnyObject>];
    }
}

/// Plays CSS's `transform: scale(from) → scale(1)` on everything the
/// window draws, about the point `fixed` (in points from the content's
/// centre, x to the right and y up), over `seconds` on the cubic bezier
/// `curve`. Core Animation scales the window's layer, so the contents scale
/// with their panel, which GPUI cannot do for an element.
pub fn pop_in(
    ns_window: &AnyObject,
    fixed: (f32, f32),
    from: f32,
    seconds: f64,
    curve: (f32, f32, f32, f32),
) {
    let (Some(animation_class), Some(timing_class)) = (
        AnyClass::get(c"CABasicAnimation"),
        AnyClass::get(c"CAMediaTimingFunction"),
    ) else {
        return;
    };
    // SAFETY: `contentView` (NSWindow) and `layer` (NSView) take nothing and
    // return an object or nil. `functionWithControlPoints::::` is a
    // CAMediaTimingFunction class method taking four floats and returning
    // the function; `animationWithKeyPath:` a CABasicAnimation class method
    // taking a string. `setFromValue:`, `setToValue:` and
    // `setTimingFunction:` take an object, `setDuration:` a
    // `CFTimeInterval` (a double), and `addAnimation:forKey:` (CALayer) an
    // animation and a string. Each receiver is checked for nil first.
    unsafe {
        let view: Option<Retained<AnyObject>> = msg_send![ns_window, contentView];
        let Some(view) = view else {
            return;
        };
        let layer: Option<Retained<AnyObject>> = msg_send![&*view, layer];
        let Some(layer) = layer else {
            return;
        };
        let timing: *mut AnyObject = timing_class.send_message(
            sel!(functionWithControlPoints::::),
            (curve.0, curve.1, curve.2, curve.3),
        );
        // A scale about a point other than the centre is the scale about the
        // centre and a shift of (1 - scale) × that point; the three run
        // together on one curve, so the point stays where it is.
        for (path, start, end) in [
            ("transform.scale", from, 1.),
            ("transform.translation.x", (1. - from) * fixed.0, 0.),
            ("transform.translation.y", (1. - from) * fixed.1, 0.),
        ] {
            let path = NSString::from_str(path);
            let animation: Option<Retained<AnyObject>> =
                msg_send![animation_class, animationWithKeyPath: &*path];
            let Some(animation) = animation else {
                return;
            };
            let (start, end) = (
                NSNumber::numberWithDouble(start.into()),
                NSNumber::numberWithDouble(end.into()),
            );
            let _: () = msg_send![&*animation, setFromValue: &*start];
            let _: () = msg_send![&*animation, setToValue: &*end];
            let _: () = msg_send![&*animation, setDuration: seconds];
            let _: () = msg_send![&*animation, setTimingFunction: timing];
            let _: () = msg_send![&*layer, addAnimation: &*animation, forKey: &*path];
        }
    }
}

/// The system's blur of what is behind a transparent window, kept to one
/// rounded panel of it. GPUI's `WindowBackgroundAppearance::Blurred` lays an
/// `NSVisualEffectView` over the whole content view, beneath GPUI's own
/// view, with the material's tint and saturation taken out, so what shows
/// is the plain blur. Here that view is hidden until [`backdrop::place`]
/// sets it to the panel's rectangle and shape. It lies inside the content
/// view, so a Core Animation transform of the content view's layer
/// ([`pop_in`]) moves it with the panel, and beneath GPUI's view, so it
/// takes no pointer.
pub mod backdrop {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2_foundation::{NSEdgeInsets, NSPoint, NSRect, NSSize};

    /// GPUI's effect view in the content view of `ns_window`.
    fn effect_view(ns_window: &AnyObject) -> Option<Retained<AnyObject>> {
        let effect = AnyClass::get(c"NSVisualEffectView")?;
        // SAFETY: `contentView` (NSWindow) and `subviews` (NSView) take
        // nothing and return an object or nil; `count` and `objectAtIndex:`
        // are NSArray's, taking nothing and an NSUInteger; `isKindOfClass:`
        // takes a class and returns a BOOL.
        unsafe {
            let content: Option<Retained<AnyObject>> = msg_send![ns_window, contentView];
            let subviews: Option<Retained<AnyObject>> = msg_send![&*content?, subviews];
            let subviews = subviews?;
            let count: usize = msg_send![&*subviews, count];
            (0..count).find_map(|index| {
                let view: Option<Retained<AnyObject>> = msg_send![&*subviews, objectAtIndex: index];
                let view = view?;
                let is_effect: Bool = msg_send![&*view, isKindOfClass: effect];
                is_effect.as_bool().then_some(view)
            })
        }
    }

    /// Hides the effect view until a panel places it. Called as the window
    /// opens, before it first shows.
    pub fn hide(ns_window: &AnyObject) {
        let Some(view) = effect_view(ns_window) else {
            return;
        };
        // SAFETY: `setHidden:` and `setAutoresizingMask:` are NSView methods
        // taking a BOOL and an NSUInteger.
        unsafe {
            let _: () = msg_send![&*view, setHidden: true];
            // It follows the panel, not the window's size.
            let _: () = msg_send![&*view, setAutoresizingMask: 0usize];
        }
    }

    /// Sets the effect view to the panel at `(x, y, w, h)` (points from the
    /// content's top left) with corners of `radius`, at `alpha`. `height` is
    /// the content's height, as AppKit counts y upwards.
    pub fn place(
        ns_window: &AnyObject,
        (x, y, w, h): (f32, f32, f32, f32),
        height: f32,
        radius: f32,
        alpha: f32,
    ) {
        let Some(view) = effect_view(ns_window) else {
            return;
        };
        let frame = NSRect::new(
            NSPoint::new(x.into(), (height - y - h).into()),
            NSSize::new(w.into(), h.into()),
        );
        // SAFETY: `setFrame:` takes an NSRect, `setAlphaValue:` a CGFloat
        // (a double), `maskImage` returns an image or nil, `setMaskImage:`
        // takes an image and `setHidden:` a BOOL: NSView and
        // NSVisualEffectView methods.
        unsafe {
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
            // SAFETY: `bezierPathWithRoundedRect:xRadius:yRadius:` takes an
            // NSRect and two CGFloats and returns a path; `blackColor` takes
            // nothing and returns a colour; `set` and `fill` take nothing.
            // AppKit calls this block on the main thread while the image
            // draws.
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
        // SAFETY: `imageWithSize:flipped:drawingHandler:` takes an NSSize, a
        // BOOL and a block of this signature and returns an image;
        // `setCapInsets:` takes an NSEdgeInsets and `setResizingMode:` an
        // `NSImageResizingMode` (an NSInteger; stretch is 1).
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
