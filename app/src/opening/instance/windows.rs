//! The instance's named pipe on Windows. The lock files are in
//! `%TEMP%\picto-gpui-instances`, the user's own; the pipe takes no remote
//! clients and is created as the first instance of its name, so a pipe of
//! that name made by another process makes the listener fail rather than
//! be joined.
use super::Request;
use std::fs::File;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::FromRawHandle as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_PIPE_CONNECTED, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAGS_AND_ATTRIBUTES, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
use windows::core::PCWSTR;

/// `%TEMP%\picto-gpui-instances`.
pub fn folder() -> std::io::Result<PathBuf> {
    let folder = std::env::temp_dir().join("picto-gpui-instances");
    std::fs::create_dir_all(&folder)?;
    Ok(folder)
}

/// The copy's canonical path, case folded (NTFS names ignore case).
pub fn identity(copy: &Path) -> std::io::Result<Vec<u8>> {
    let canonical = copy.canonicalize()?;
    Ok(canonical.to_string_lossy().to_lowercase().into_bytes())
}

/// The pipe's name: the key and the user, since pipe names are shared by
/// every user of the machine.
fn pipe(key: &str) -> Vec<u16> {
    let user = std::env::var("USERNAME").unwrap_or_default();
    let mut user_hash: u64 = 0xcbf29ce484222325;
    for byte in user.to_lowercase().bytes() {
        user_hash ^= u64::from(byte);
        user_hash = user_hash.wrapping_mul(0x100000001b3);
    }
    let name = format!(r"\\.\pipe\picto-gpui-{user_hash:016x}-{key}");
    std::ffi::OsStr::new(&name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn create(name: &[u16], first: bool) -> std::io::Result<HANDLE> {
    let mode = PIPE_ACCESS_DUPLEX
        | if first {
            FILE_FLAG_FIRST_PIPE_INSTANCE
        } else {
            FILE_FLAGS_AND_ATTRIBUTES(0)
        };
    // SAFETY: `name` is a nul-terminated wide string; no security
    // attributes, so the default ones (the creator and administrators
    // write, others read) apply.
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            4096,
            4096,
            0,
            None,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    Ok(handle)
}

/// Listens for launches on the copy: one pipe instance at a time waits for
/// a launch, and each connected one is answered on a thread of its own.
pub fn serve(
    _folder: &Path,
    key: &str,
    deliver: Arc<dyn Fn(Request) + Send + Sync>,
) -> std::io::Result<()> {
    let name = pipe(key);
    let first = create(&name, true)?.0 as usize;
    std::thread::Builder::new()
        .name("instance".into())
        .spawn(move || {
            let mut waiting = HANDLE(first as _);
            loop {
                // SAFETY: `waiting` is a pipe instance this thread made and
                // owns; no overlapped structure, so the call blocks.
                let connected = unsafe { ConnectNamedPipe(waiting, None) }.is_ok()
                    || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
                if connected {
                    // SAFETY: the handle is a connected pipe this thread
                    // owns; the file takes it and closes it when dropped.
                    let stream = unsafe { File::from_raw_handle(waiting.0) };
                    let deliver = deliver.clone();
                    std::thread::spawn(move || super::answer(stream, &*deliver));
                } else {
                    // SAFETY: the handle is ours and not used again.
                    let _ = unsafe { CloseHandle(waiting) };
                }
                match create(&name, false) {
                    Ok(next) => waiting = next,
                    Err(_) => return,
                }
            }
        })?;
    Ok(())
}

pub fn connect(_folder: &Path, key: &str) -> std::io::Result<File> {
    let name = pipe(key);
    let name = String::from_utf16_lossy(&name[..name.len() - 1]);
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(name)
}

/// Lets the instance bring its window in front of this launch's, which the
/// system otherwise refuses to a process that is not in front.
pub fn allow_foreground(pid: u32) {
    // SAFETY: takes a process id; failing leaves the window where it is.
    let _ = unsafe { AllowSetForegroundWindow(pid) };
}
