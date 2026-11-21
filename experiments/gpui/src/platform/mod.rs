//! The calls into each system the app makes for what GPUI has no option
//! for, one module per system. The UI layer reaches them through one shared
//! function each (`ui::window_chrome`), which does nothing where a system
//! has no such call.
/// The sign-in window of site logins, one interface over each system's web
/// view.
pub mod login_window;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

/// Removes a temporary folder another process may still hold (a browser's
/// profile, a web view's data): at once if it can be, else again every
/// half second for ten seconds on a thread of its own while that process
/// lets go of its files (Windows keeps them locked for a moment after the
/// process ends). `what` names the folder in the log if it stays.
pub fn remove_when_released(folder: tempfile::TempDir, what: &'static str) {
    let path = folder.keep();
    let removed = |path: &std::path::Path| std::fs::remove_dir_all(path).is_ok() || !path.exists();
    if removed(&path) {
        return;
    }
    std::thread::spawn(move || {
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if removed(&path) {
                return;
            }
        }
        eprintln!("picto: {what} could not be removed: {}", path.display());
    });
}
