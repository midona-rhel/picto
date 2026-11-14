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
    if enabled() {
        use crate::platform::macos;
        macos::with_ns_window(window, macos::keep_painting_while_covered);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn keep_painting(_: &Window) {}
