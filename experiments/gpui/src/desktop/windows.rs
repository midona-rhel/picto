use super::Application;
use std::ffi::c_void;
use std::path::{Path, PathBuf};

pub fn set_badge(_count: u64) {}

pub fn notify(title: &str, body: &str, cx: &gpui::App) {
    super::system_notification(title, body, cx);
}

#[link(name = "user32")]
unsafe extern "system" {
    fn OpenClipboard(owner: *mut c_void) -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, memory: *mut c_void) -> *mut c_void;
    fn CloseClipboard() -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> *mut c_void;
    fn GlobalLock(memory: *mut c_void) -> *mut c_void;
    fn GlobalUnlock(memory: *mut c_void) -> i32;
    fn GlobalFree(memory: *mut c_void) -> *mut c_void;
}

/// `CF_HDROP`, and `GHND` (moveable, zeroed).
const CF_HDROP: u32 = 15;
const GHND: u32 = 0x0042;

/// `copy_files` in `native/picto-node`: a `DROPFILES` list of the paths
/// on the clipboard, which Explorer pastes as the files.
pub fn copy_files(paths: &[PathBuf]) -> Result<(), String> {
    let payload = super::drop_files(paths);
    // SAFETY: the clipboard and global-memory calls with the arguments
    // they take; the memory is handed to the system only when
    // `SetClipboardData` takes it, and freed here otherwise.
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("Could not open the system clipboard".into());
        }
        EmptyClipboard();
        let memory = GlobalAlloc(GHND, payload.len());
        if memory.is_null() {
            CloseClipboard();
            return Err("Could not allocate a file clipboard payload".into());
        }
        let locked = GlobalLock(memory).cast::<u8>();
        if locked.is_null() {
            GlobalFree(memory);
            CloseClipboard();
            return Err("Could not prepare a file clipboard payload".into());
        }
        std::ptr::copy_nonoverlapping(payload.as_ptr(), locked, payload.len());
        GlobalUnlock(memory);
        if SetClipboardData(CF_HDROP, memory).is_null() {
            GlobalFree(memory);
            CloseClipboard();
            return Err("Could not write files to the system clipboard".into());
        }
        CloseClipboard();
    }
    Ok(())
}

/// `picto:shell:openWithChooser`: Windows' "Open with" dialog on the
/// file, from which the user picks the application.
pub fn open_with_chooser(file: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt as _;
    /// `DETACHED_PROCESS`: the chooser outlives nothing of ours.
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    std::process::Command::new("rundll32.exe")
        .arg("shell32.dll,OpenAs_RunDLL")
        .arg(file)
        .creation_flags(DETACHED_PROCESS)
        .spawn()
        .map(drop)
        .map_err(|error| format!("Could not open the application chooser: {error}"))
}

/// Windows offers its own chooser instead of a list.
pub fn applications(_file: &Path) -> Vec<Application> {
    Vec::new()
}
