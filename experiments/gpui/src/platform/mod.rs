//! The calls into each system the app makes for what GPUI has no option
//! for, one module per system. The UI layer reaches them through one shared
//! function each (`ui::window_chrome`), which does nothing where a system
//! has no such call.
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;
