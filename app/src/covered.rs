//! Keeps the app's windows drawing while other windows cover them, when the
//! environment variable `PICTO_GPUI_BACKGROUND` is set. GPUI stops drawing a
//! window's frames once AppKit reports it fully covered, so a capture of it
//! shows whatever it last drew. With the variable set, GPUI's window class
//! answers that its windows are always visible, so the app can be checked
//! behind other applications without being brought forward. The answer is
//! replaced for the whole class, so it also holds for every window the app
//! opens afterwards, until the app quits. Nothing changes when the variable
//! is unset. A release build never reads the variable (see `seams.rs`).
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
