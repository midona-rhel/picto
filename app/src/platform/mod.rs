//! The calls that the app makes into the operating system for things that
//! GPUI has no option for. Each operating system has a module of its own.
//! The interface code does not call these modules directly: it calls the
//! functions of `ui::window_chrome`, each of which calls the module of the
//! current system and does nothing on a system that has no such call.
/// The sign-in window for site logins. Its interface is the same on every
/// system, and it shows the web view of the system it runs on.
pub mod login_window;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

/// Removes a temporary folder that another process may still have open,
/// such as the profile of a browser or the data of a web view. If the
/// folder cannot be removed immediately, a thread tries again every half
/// second for ten seconds. Windows keeps the files of a process locked for
/// a moment after the process has ended. If the folder is still there
/// after the ten seconds, a line with `what` and the path is written to
/// standard error.
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
