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

#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteW(
        window: *mut c_void,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> *mut c_void;
}

/// `SW_SHOWNORMAL`
const SW_SHOWNORMAL: i32 = 1;

fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt as _;
    text.encode_wide().chain([0]).collect()
}

/// `ShellExecuteW`: `file` with `parameters` by `operation` (`runas`
/// asks for administrator rights, `open` is the file's default action).
fn shell_execute(operation: &str, file: &Path, parameters: &str) -> Result<(), String> {
    let (operation, path, parameters) = (
        wide(operation.as_ref()),
        wide(file.as_os_str()),
        wide(parameters.as_ref()),
    );
    // SAFETY: every pointer is a NUL-terminated wide string that outlives
    // the call; no window is named.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            path.as_ptr(),
            parameters.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as usize;
    // Above 32 is success; below, the error.
    if result > 32 {
        Ok(())
    } else {
        Err(format!(
            "{} could not be started (ShellExecute error {result})",
            file.display()
        ))
    }
}

/// `NsisUpdater.doInstall`: the NSIS installer started with `--updated`,
/// `/S` when silent and `--force-run` to start the app again afterwards,
/// detached and without standard streams. Elevated (production's
/// `elevate.exe`, here the system's `runas`) when the feed says it needs
/// administrator rights or Windows refuses it without them; opened as a
/// file (`shell.openPath`) when it cannot be found to run.
pub fn install_update(
    installer: &Path,
    silent: bool,
    run_after: bool,
    admin: bool,
) -> Result<(), String> {
    use std::os::windows::process::CommandExt as _;
    use std::process::{Command, Stdio};
    /// `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`: Node's `detached`.
    const DETACHED: u32 = 0x0000_0008 | 0x0000_0200;
    let mut arguments = vec!["--updated"];
    if silent {
        arguments.push("/S");
    }
    if run_after {
        arguments.push("--force-run");
    }
    let elevated = || shell_execute("runas", installer, &arguments.join(" "));
    if admin {
        return elevated();
    }
    match Command::new(installer)
        .args(&arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED)
        .spawn()
    {
        Ok(_) => Ok(()),
        // `EACCES` / `UNKNOWN`: access denied, or elevation required.
        Err(error) if matches!(error.raw_os_error(), Some(5 | 740)) => elevated(),
        // `ENOENT`
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            shell_execute("open", installer, "")
        }
        Err(error) => Err(format!("spawn {} {error}", installer.display())),
    }
}

/// Only Linux runs from an AppImage.
pub fn app_image() -> Option<PathBuf> {
    None
}
