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

/// Puts a `DROPFILES` list of the paths on the clipboard, in the `CF_HDROP`
/// format. Explorer pastes the files that the list names.
pub fn copy_files(paths: &[PathBuf]) -> Result<(), String> {
    let payload = super::drop_files(paths);
    // SAFETY: The clipboard functions and the global-memory functions are
    // called with the arguments they document. The memory is allocated with
    // `GHND`, which makes it moveable and fills it with zeros. The memory
    // belongs to the system once `SetClipboardData` has accepted it. In every
    // other case it is freed here.
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

/// Opens Windows' "Open with" dialog for the file. The user picks the
/// application in that dialog.
pub fn open_with_chooser(file: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt as _;
    /// The process creation flag `DETACHED_PROCESS`: `rundll32.exe` does not
    /// get this process's console.
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    std::process::Command::new("rundll32.exe")
        .arg("shell32.dll,OpenAs_RunDLL")
        .arg(file)
        .creation_flags(DETACHED_PROCESS)
        .spawn()
        .map(drop)
        .map_err(|error| format!("Could not open the application chooser: {error}"))
}

/// Returns no applications: on Windows the user picks the application in
/// Windows' own dialog ([`open_with_chooser`]).
pub fn applications(_file: &Path) -> Vec<Application> {
    Vec::new()
}

fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt as _;
    text.encode_wide().chain([0]).collect()
}

/// Starts `file` with `parameters` through `ShellExecuteW`. The operation
/// `runas` asks the user for administrator rights, and the operation `open`
/// does the file's default action.
fn shell_execute(operation: &str, file: &Path, parameters: &str) -> Result<(), String> {
    let (operation, path, parameters) = (
        wide(operation.as_ref()),
        wide(file.as_os_str()),
        wide(parameters.as_ref()),
    );
    // SAFETY: Every pointer is to a NUL-terminated UTF-16 string that lives
    // until the call returns. No parent window is given.
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
    // `ShellExecuteW` returns a value above 32 when it succeeded, and an error
    // code of 32 or less when it failed.
    if result > 32 {
        Ok(())
    } else {
        Err(format!(
            "{} could not be started (ShellExecute error {result})",
            file.display()
        ))
    }
}

/// Starts the NSIS installer of an update. The installer gets the argument
/// `--updated`, the argument `/S` when the installation is silent, and the
/// argument `--force-run` when the app is to be started again afterwards. It
/// is started as a detached process without standard streams.
///
/// The installer is started with administrator rights, through the system's
/// `runas` operation, when `admin` is set or when Windows refuses to start it
/// without them. `admin` is set when the update feed says that the installer
/// needs administrator rights. If Windows does not find the installer as a
/// program to run, the installer is opened as a file.
pub fn install_update(
    installer: &Path,
    silent: bool,
    run_after: bool,
    admin: bool,
) -> Result<(), String> {
    use std::os::windows::process::CommandExt as _;
    use std::process::{Command, Stdio};
    /// The process creation flags `DETACHED_PROCESS` and
    /// `CREATE_NEW_PROCESS_GROUP`: the installer does not get this process's
    /// console and is in a process group of its own, so that neither closing
    /// that console nor Ctrl+C in it ends the installer.
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
        // Windows error 5 says that access is denied, and error 740 says that
        // the operation requires elevation.
        Err(error) if matches!(error.raw_os_error(), Some(5 | 740)) => elevated(),
        // The installer was not found as a program to run.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            shell_execute("open", installer, "")
        }
        Err(error) => Err(format!("spawn {} {error}", installer.display())),
    }
}

/// Returns `None`: only the Linux build runs from an AppImage.
pub fn app_image() -> Option<PathBuf> {
    None
}
