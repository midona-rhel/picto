//! Opt-in: keep drawing while the window is covered. GPUI stops a window's
//! frames once AppKit reports it fully covered, so a capture of it shows
//! whatever it last drew. With `PICTO_GPUI_BACKGROUND` set the window is told
//! it is always visible, and can be checked behind other applications
//! without being brought forward. Nothing changes when the variable is unset.
use gpui::Window;

pub fn enabled() -> bool {
    std::env::var_os("PICTO_GPUI_BACKGROUND").is_some()
}

#[cfg(target_os = "macos")]
pub fn keep_painting(window: &Window) {
    use std::ffi::{c_char, c_void};
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
    /// `NSWindowOcclusionStateVisible`.
    extern "C" fn visible(_: *mut c_void, _: *const c_void) -> usize {
        1 << 1
    }
    if !enabled() {
        return;
    }
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return;
    };
    // SAFETY: `ns_view` is the live content view of this window. GPUI's
    // window class is its own delegate and implements
    // `windowDidChangeOcclusionState:`; `occlusionState` returns an
    // `NSUInteger` and takes no arguments. Called on the main thread.
    unsafe {
        let send_id: extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let send_object: extern "C" fn(*mut c_void, *const c_void, *mut c_void) =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let ns_window = send_id(
            appkit.ns_view.as_ptr(),
            sel_registerName(c"window".as_ptr()),
        );
        if ns_window.is_null() {
            return;
        }
        class_replaceMethod(
            object_getClass(ns_window),
            sel_registerName(c"occlusionState".as_ptr()),
            visible as *const c_void,
            c"Q@:".as_ptr(),
        );
        // Have the window look at its (new) state and start its frames.
        send_object(
            ns_window,
            sel_registerName(c"windowDidChangeOcclusionState:".as_ptr()),
            std::ptr::null_mut(),
        );
    }
}

#[cfg(not(target_os = "macos"))]
pub fn keep_painting(_: &Window) {}
