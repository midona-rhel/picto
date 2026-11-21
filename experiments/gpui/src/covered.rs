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
    if crate::seams::background() {
        use crate::platform::macos;
        macos::with_ns_window(window, macos::keep_painting_while_covered);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn keep_painting(_: &Window) {}
