//! The environment variables the checks use to drive and observe the app
//! (README, "Checks"). Each is read in debug builds only: a release build
//! answers as though none were set, and holds neither their names nor the
//! code behind them, so nothing that can set a shipped app's environment can
//! make it take input from a file, keep painting while covered, hold its
//! network commands or keep its state somewhere else.
//!
//! Every build reads the owner's debugging aids, `PICTO_GPUI_FPS`
//! (`meter.rs`) and `PICTO_GPUI_STATS` (`stats.rs`), and the variables that
//! configure the app on any machine (`PICTO_FFMPEG_DIR`, the platform's
//! own). The off-screen tool's variables (`offscreen.rs`) and the tests'
//! exist in test builds only.
use std::path::PathBuf;

/// The variable's value in a debug build; nothing in a release build, which
/// does not hold the name.
macro_rules! seam {
    ($name:literal) => {{
        #[cfg(debug_assertions)]
        let value = std::env::var_os($name);
        #[cfg(not(debug_assertions))]
        let value: Option<std::ffi::OsString> = None;
        value
    }};
}

/// `PICTO_GPUI_POINTER=<file or FIFO>`: input lines to play into the main
/// window (`pointer.rs`).
#[cfg(debug_assertions)]
pub fn pointer_file() -> Option<PathBuf> {
    seam!("PICTO_GPUI_POINTER").map(PathBuf::from)
}

/// `PICTO_GPUI_BACKGROUND`: the main window opens without the keyboard and
/// keeps drawing while covered (`covered.rs`).
pub fn background() -> bool {
    seam!("PICTO_GPUI_BACKGROUND").is_some()
}

/// `PICTO_GPUI_WINDOW=<width>x<height>`: the main window opens centred at
/// that size, which is then not kept, so a picture compares pixel for pixel
/// with the Electron window's.
pub fn exact_window() -> Option<(f32, f32)> {
    window_size(&seam!("PICTO_GPUI_WINDOW")?.to_string_lossy())
}

/// `1213x865` as a width and a height.
fn window_size(value: &str) -> Option<(f32, f32)> {
    let (width, height) = value.split_once('x')?;
    Some((width.trim().parse().ok()?, height.trim().parse().ok()?))
}

/// `PICTO_GPUI_MENU=window`: pop-up menus are drawn inside the window on
/// the Mac too (`ui::menu::presentation`).
pub fn menus_in_window() -> bool {
    seam!("PICTO_GPUI_MENU").is_some_and(|value| value == "window")
}

/// `PICTO_GPUI_APP_MENU=window`: the window carries the application menu
/// button on the Mac too (`app_menu::in_window`).
pub fn app_menu_in_window() -> bool {
    seam!("PICTO_GPUI_APP_MENU").is_some_and(|value| value == "window")
}

/// `PICTO_GPUI_STATE=<folder>`: what the user arranges is kept there
/// instead of in the app's own folder (`state.rs`).
pub fn state_folder() -> Option<PathBuf> {
    seam!("PICTO_GPUI_STATE")
        .filter(|folder| !folder.is_empty())
        .map(PathBuf::from)
}

/// `PICTO_GPUI_INSPECTOR=<width>`: the inspector opens at that width
/// whatever was kept.
pub fn inspector_width() -> Option<f32> {
    seam!("PICTO_GPUI_INSPECTOR")?.to_str()?.trim().parse().ok()
}

/// `PICTO_GPUI_HOLD_RUNS`: the Subscriptions screen writes its commands
/// down and answers those that would start network work instead of sending
/// them.
pub fn hold_runs() -> bool {
    seam!("PICTO_GPUI_HOLD_RUNS").is_some()
}

/// `PICTO_GPUI_SILENT`: every player and Flash movie starts muted.
pub fn silent() -> bool {
    seam!("PICTO_GPUI_SILENT").is_some()
}

#[cfg(test)]
mod tests {
    use super::window_size;

    #[test]
    fn a_window_size_is_a_width_by_a_height() {
        assert_eq!(window_size("1213x865"), Some((1213., 865.)));
        assert_eq!(window_size(" 800 x 600 "), Some((800., 600.)));
        assert_eq!(window_size("1213"), None);
        assert_eq!(window_size("wide x tall"), None);
    }
}
