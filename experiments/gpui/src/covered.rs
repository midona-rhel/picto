//! Opt-in: keep drawing while the window is covered. GPUI stops a window's
//! frames once AppKit reports it fully covered, so a capture of it shows
//! whatever it last drew. With `PICTO_GPUI_BACKGROUND` set, GPUI's window
//! class answers that its windows are always visible, so the app can be
//! checked behind other applications without being brought forward. The
//! answer is replaced for the class, so it holds for every window the app
//! opens afterwards too, until the app quits. Nothing changes when the
//! variable is unset, which a release build never reads (see `seams.rs`).
use gpui::Window;

#[cfg(target_os = "macos")]
pub fn keep_painting(window: &Window) {
    use crate::ui::window_chrome::appkit;
    use std::ffi::c_void;
    /// `NSWindowOcclusionStateVisible`.
    extern "C" fn visible(_: *mut c_void, _: *const c_void) -> usize {
        1 << 1
    }
    if !crate::seams::background() {
        return;
    }
    appkit::with_ns_window(window, |ns_window| {
        // SAFETY: GPUI's window class is its own delegate and implements
        // `windowDidChangeOcclusionState:`; `occlusionState` returns an
        // `NSUInteger` and takes no arguments, as `visible` does. Called on
        // the main thread.
        unsafe {
            appkit::replace_method(
                ns_window,
                appkit::selector(c"occlusionState"),
                visible as *const c_void,
                c"Q@:",
            );
            // Have the window look at its (new) state and start its frames.
            let send_object: extern "C" fn(*mut c_void, *const c_void, *mut c_void) =
                std::mem::transmute(appkit::send());
            send_object(
                ns_window,
                appkit::selector(c"windowDidChangeOcclusionState:"),
                std::ptr::null_mut(),
            );
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub fn keep_painting(_: &Window) {}
