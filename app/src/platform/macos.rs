//! The AppKit and Core Animation calls that the app makes for things that
//! GPUI has no option for. They are made through `objc2`. In a debug build
//! its `msg_send!` checks the argument types and the return type of each
//! message against those of the method.
//!
//! The functions that act on a window take the window's `NSWindow`, which
//! [`with_ns_window`] provides. Every function runs on the main thread,
//! which is the thread on which GPUI calls the app's code. AppKit requires
//! both.
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
    // SAFETY: `ns_view` is the view that GPUI made for this open window
    // (the handle borrows the window, which owns it). `window` is an NSView
    // method that takes no arguments and returns the view's NSWindow or nil.
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
    // The values 0, 1 and 2 are `NSWindowCloseButton`,
    // `NSWindowMiniaturizeButton` and `NSWindowZoomButton`.
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

/// Puts the window at the level above ordinary windows
/// (`NSFloatingWindowLevel`), so that it stays in front of them, or back at
/// the ordinary level (`NSNormalWindowLevel`).
pub fn set_floating(ns_window: &AnyObject, floating: bool) {
    let level: isize = if floating { 3 } else { 0 };
    // SAFETY: `setLevel:` is an NSWindow method taking an `NSWindowLevel`
    // (an NSInteger).
    unsafe {
        let _: () = msg_send![ns_window, setLevel: level];
    }
}

/// Hides the window without closing it (`orderOut:`).
pub fn order_out(ns_window: &AnyObject) {
    // SAFETY: `orderOut:` is an NSWindow method taking its sender, an
    // object that may be nil.
    unsafe {
        let _: () = msg_send![ns_window, orderOut: None::<&AnyObject>];
    }
}

/// Puts the window in front of the app's other windows (`orderFront:`).
/// The window does not become the key window, and the app is not brought
/// in front of other apps.
pub fn order_front(ns_window: &AnyObject) {
    // SAFETY: `orderFront:` is an NSWindow method taking its sender, an
    // object that may be nil.
    unsafe {
        let _: () = msg_send![ns_window, orderFront: None::<&AnyObject>];
    }
}

/// Closes the window the way its close button does (`performClose:`):
/// AppKit first asks the window whether it may close, and then closes it.
/// The message is not sent now. It is scheduled on the run loop, which is
/// also where a click on the button is handled, because GPUI can call the
/// window's close handler only while the app is not borrowed, and the
/// caller of this function borrows it.
#[cfg(debug_assertions)]
pub fn perform_close(ns_window: &AnyObject) {
    // SAFETY: `performSelector:withObject:afterDelay:` is an NSObject
    // method that takes a selector, the selector's argument (here nil, the
    // sender of `performClose:`) and a delay. The window is retained until
    // the message is sent.
    unsafe {
        let _: () = msg_send![
            ns_window,
            performSelector: sel!(performClose:),
            withObject: None::<&AnyObject>,
            afterDelay: 0.0f64
        ];
    }
}

/// Returns the number that the window server gave the window
/// (`windowNumber`). `screencapture -l` takes this number.
#[cfg(debug_assertions)]
pub fn window_number(ns_window: &AnyObject) -> isize {
    // SAFETY: `windowNumber` is an NSWindow method that takes nothing and
    // returns an NSInteger.
    unsafe { msg_send![ns_window, windowNumber] }
}

/// Shows the window behind every other window (`orderBack:`).
pub fn order_back(ns_window: &AnyObject) {
    // SAFETY: `orderBack:` is an NSWindow method taking its sender, an
    // object that may be nil.
    unsafe {
        let _: () = msg_send![ns_window, orderBack: None::<&AnyObject>];
    }
}

/// Turns off the shadow and the light line that the system draws around
/// the edge of the window's content.
pub fn without_shadow(ns_window: &AnyObject) {
    // SAFETY: `setHasShadow:` is an NSWindow method taking a BOOL.
    unsafe {
        let _: () = msg_send![ns_window, setHasShadow: false];
    }
}

/// Turns off the animation that AppKit plays when a window appears, in
/// which a panel grows from a smaller size. It is used for windows that
/// play an animation of their own.
pub fn without_appear_animation(ns_window: &AnyObject) {
    /// The value of `NSWindowAnimationBehaviorNone`.
    const NONE: isize = 2;
    // SAFETY: `setAnimationBehavior:` is an NSWindow method taking an
    // `NSWindowAnimationBehavior` (an NSInteger).
    unsafe {
        let _: () = msg_send![ns_window, setAnimationBehavior: NONE];
    }
}

/// Stops AppKit from adding its own row "Enter Full Screen" to the View
/// menu, which already has the app's row "Toggle Full Screen". It does so
/// by registering a default value for `NSFullScreenMenuItemEverywhere`,
/// which AppKit reads when the menu bar is first shown. A registered
/// default is not written to the user's preferences.
pub fn one_full_screen_row() {
    let (Some(defaults), Some(dictionary)) = (
        AnyClass::get(c"NSUserDefaults"),
        AnyClass::get(c"NSDictionary"),
    ) else {
        return;
    };
    let key = NSString::from_str("NSFullScreenMenuItemEverywhere");
    let no = NSNumber::new_bool(false);
    // SAFETY: `standardUserDefaults` takes no arguments and returns the
    // shared object. `dictionaryWithObject:forKey:` takes two objects and
    // returns a dictionary. `registerDefaults:` takes that dictionary.
    unsafe {
        let defaults: Option<Retained<AnyObject>> = msg_send![defaults, standardUserDefaults];
        let values: Option<Retained<AnyObject>> =
            msg_send![dictionary, dictionaryWithObject: &*no, forKey: &*key];
        if let (Some(defaults), Some(values)) = (defaults, values) {
            let _: () = msg_send![&*defaults, registerDefaults: &*values];
        }
    }
}

/// Brings all windows of the app in front of the windows of other apps and
/// keeps their order among themselves (`[NSApp arrangeInFront:nil]`).
pub fn arrange_in_front() {
    let Some(class) = AnyClass::get(c"NSApplication") else {
        return;
    };
    // SAFETY: `sharedApplication` is an NSApplication class method that
    // takes no arguments and returns the app. `arrangeInFront:` takes its
    // sender, an object that may be nil.
    unsafe {
        let app: Option<Retained<AnyObject>> = msg_send![class, sharedApplication];
        if let Some(app) = app {
            let _: () = msg_send![&*app, arrangeInFront: None::<&AnyObject>];
        }
    }
}

/// Makes the window always report that it is visible. GPUI stops drawing a
/// window's frames when AppKit reports that the window is covered. After
/// this call GPUI keeps drawing the window while the windows of other
/// applications cover it.
pub fn keep_painting_while_covered(ns_window: &AnyObject) {
    /// Returns `NSWindowOcclusionStateVisible`, whether or not anything
    /// covers the window.
    extern "C-unwind" fn visible(_: &AnyObject, _: Sel) -> usize {
        1 << 1
    }
    let implementation: extern "C-unwind" fn(&AnyObject, Sel) -> usize = visible;
    // SAFETY: the class is the class of this window, which is GPUI's
    // window class and also the window's delegate. `occlusionState` takes no
    // arguments and returns an NSUInteger, as `visible` does, and "Q@:"
    // encodes that signature. `windowDidChangeOcclusionState:` is the
    // delegate method that GPUI's class implements; it takes a notification
    // (here nil) and returns nothing.
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
        // The window is told that its occlusion state changed, so that
        // GPUI reads the new state and starts drawing frames.
        let _: () = msg_send![ns_window, windowDidChangeOcclusionState: None::<&AnyObject>];
    }
}

/// Animates the scale of everything the window draws from `from` to 1.
/// The point `fixed` stays where it is during the animation; it is given in
/// points from the centre of the content, with x to the right and y up. The
/// animation lasts `seconds` and follows the cubic bezier `curve`. Core
/// Animation scales the layer of the window's content view, so a panel and
/// everything inside it are scaled together. GPUI cannot scale an element
/// with its children.
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
    // SAFETY: `contentView` (NSWindow) and `layer` (NSView) take no
    // arguments and return an object or nil.
    // `functionWithControlPoints::::` is a CAMediaTimingFunction class
    // method that takes four floats and returns the function.
    // `animationWithKeyPath:` is a CABasicAnimation class method that takes
    // a string. `setFromValue:`, `setToValue:` and `setTimingFunction:`
    // take an object, `setDuration:` takes a `CFTimeInterval` (a double),
    // and `addAnimation:forKey:` (CALayer) takes an animation and a string.
    // Each receiver is checked for nil before a message is sent to it.
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
        // The layer scales about its centre. A scale about another point is
        // the scale about the centre together with a shift of
        // (1 - scale) × that point. The scale and the two shifts run for the
        // same time on the same curve, so the point `fixed` does not move.
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

/// Limits the system's blur of what is behind a transparent window to one
/// rounded panel of the window.
///
/// With `WindowBackgroundAppearance::Blurred`, GPUI puts an
/// `NSVisualEffectView` over the whole content view, below GPUI's own view,
/// and removes the tint and the saturation of the material, so the view
/// shows only the blur. This module hides that view until
/// [`backdrop::place`] gives it the rectangle and the rounded corners of
/// the panel. The view is inside the content view, so the animation of
/// [`pop_in`], which transforms the layer of the content view, moves it
/// together with the panel. The view is below GPUI's view, so it receives
/// no pointer events.
pub mod backdrop {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2_foundation::{NSEdgeInsets, NSPoint, NSRect, NSSize};

    /// Returns the `NSVisualEffectView` that GPUI put in the content view
    /// of `ns_window`.
    fn effect_view(ns_window: &AnyObject) -> Option<Retained<AnyObject>> {
        let effect = AnyClass::get(c"NSVisualEffectView")?;
        // SAFETY: `contentView` (NSWindow) and `subviews` (NSView) take no
        // arguments and return an object or nil. `count` and
        // `objectAtIndex:` are NSArray methods; the first takes no
        // arguments and the second an NSUInteger. `isKindOfClass:` takes a
        // class and returns a BOOL.
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

    /// Hides the effect view until [`place`] shows it. It is called when
    /// the window opens, before the window is first shown.
    pub fn hide(ns_window: &AnyObject) {
        let Some(view) = effect_view(ns_window) else {
            return;
        };
        // SAFETY: `setHidden:` and `setAutoresizingMask:` are NSView methods
        // that take a BOOL and an NSUInteger.
        unsafe {
            let _: () = msg_send![&*view, setHidden: true];
            // The view must not resize with the window, because `place`
            // sets its frame to the rectangle of the panel.
            let _: () = msg_send![&*view, setAutoresizingMask: 0usize];
        }
    }

    /// Shows the effect view at the rectangle `(x, y, w, h)` of the panel,
    /// given in points from the top left of the content, with rounded
    /// corners of `radius` and with the opacity `alpha`. `height` is the
    /// height of the content; it is needed because AppKit counts y from
    /// the bottom.
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
        // SAFETY: these are methods of NSView and NSVisualEffectView.
        // `setFrame:` takes an NSRect, `setAlphaValue:` a CGFloat (a
        // double), `maskImage` returns an image or nil, `setMaskImage:`
        // takes an image, and `setHidden:` takes a BOOL.
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

    /// Returns an image of a rounded rectangle with corners of `radius`
    /// that can be stretched to any size: its cap insets keep the corners
    /// as they are and stretch only the middle. The documentation of
    /// `NSVisualEffectView.maskImage` describes this for rounded shapes.
    fn rounded_mask(radius: f64) -> Option<Retained<AnyObject>> {
        let (image, path, color) = (
            AnyClass::get(c"NSImage")?,
            AnyClass::get(c"NSBezierPath")?,
            AnyClass::get(c"NSColor")?,
        );
        let side = radius * 2. + 1.;
        let draw = block2::RcBlock::new(move |rect: NSRect| -> Bool {
            // SAFETY: `bezierPathWithRoundedRect:xRadius:yRadius:` takes an
            // NSRect and two CGFloats and returns a path. `blackColor` takes
            // no arguments and returns a colour. `set` and `fill` take no
            // arguments. AppKit calls this block on the main thread while
            // it draws the image.
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
        // BOOL and a block of this signature and returns an image.
        // `setCapInsets:` takes an NSEdgeInsets. `setResizingMode:` takes
        // an `NSImageResizingMode`, an NSInteger whose value 1 is stretch.
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
