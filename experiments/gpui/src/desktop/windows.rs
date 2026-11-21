use super::Application;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{GlobalFree, HANDLE};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GHND, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_HDROP;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

pub fn set_badge(_count: u64) {}

pub fn notify(title: &str, body: &str, cx: &gpui::App) {
    super::system_notification(title, body, cx);
}

/// `copy_files` in `native/picto-node`: a `DROPFILES` list of the paths
/// on the clipboard, which Explorer pastes as the files.
pub fn copy_files(paths: &[PathBuf]) -> Result<(), String> {
    let payload = super::drop_files(paths);
    // SAFETY: the clipboard and global-memory calls with the arguments
    // they take; the memory is handed to the system only when
    // `SetClipboardData` takes it, and freed here otherwise.
    // (GHND: moveable and zeroed.)
    unsafe {
        if OpenClipboard(None).is_err() {
            return Err("Could not open the system clipboard".into());
        }
        let _ = EmptyClipboard();
        let Ok(memory) = GlobalAlloc(GHND, payload.len()) else {
            let _ = CloseClipboard();
            return Err("Could not allocate a file clipboard payload".into());
        };
        let locked = GlobalLock(memory).cast::<u8>();
        if locked.is_null() {
            let _ = GlobalFree(Some(memory));
            let _ = CloseClipboard();
            return Err("Could not prepare a file clipboard payload".into());
        }
        std::ptr::copy_nonoverlapping(payload.as_ptr(), locked, payload.len());
        let _ = GlobalUnlock(memory);
        if SetClipboardData(u32::from(CF_HDROP.0), Some(HANDLE(memory.0))).is_err() {
            let _ = GlobalFree(Some(memory));
            let _ = CloseClipboard();
            return Err("Could not write files to the system clipboard".into());
        }
        let _ = CloseClipboard();
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
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(path.as_ptr()),
            PCWSTR(parameters.as_ptr()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    }
    .0 as usize;
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
