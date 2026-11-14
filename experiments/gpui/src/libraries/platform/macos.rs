//! Whether the Electron app runs, on macOS: every process's executable path
//! (`proc_listallpids`, `proc_pidpath`), looked at by [`is_picto`].
use std::ffi::c_void;
use std::path::Path;

/// The executable of the first process of the Electron app found, if any.
pub fn picto_process() -> Result<Option<String>, String> {
    // SAFETY: a null buffer asks for the number of processes.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Err("the list of processes could not be read".into());
    }
    // Room for processes started since the count.
    let mut pids = vec![0 as libc::c_int; count as usize + 64];
    let bytes = (pids.len() * std::mem::size_of::<libc::c_int>()) as libc::c_int;
    // SAFETY: the buffer holds `bytes` bytes.
    let found = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast::<c_void>(), bytes) };
    if found <= 0 {
        return Err("the list of processes could not be read".into());
    }
    let own = std::process::id() as libc::c_int;
    for &pid in pids.iter().take(found as usize) {
        if pid <= 0 || pid == own {
            continue;
        }
        let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: the buffer holds `path.len()` bytes.
        let length = unsafe {
            libc::proc_pidpath(pid, path.as_mut_ptr().cast::<c_void>(), path.len() as u32)
        };
        // A process of another user, or one that ended, has no path here.
        if length <= 0 {
            continue;
        }
        let path = String::from_utf8_lossy(&path[..length as usize]).into_owned();
        if is_picto(&path) {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// Whether an executable is the Electron app's: its main process or one of
/// its helpers, inside any `Picto.app` (production's `productName`), or an
/// executable named `Picto` wherever the bundle was renamed to. This app's
/// own bundle (`Picto GPUI.app`, executable `picto-gpui-spike`) is not.
pub fn is_picto(path: &str) -> bool {
    if path.contains("/Picto.app/Contents/") {
        return true;
    }
    let name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    // i18n: a process's name
    name == "Picto" || name.starts_with("Picto Helper")
}

/// The database file's identity: its device and inode.
pub fn file_identity(path: &Path) -> std::io::Result<String> {
    super::unix_identity(path)
}

#[cfg(test)]
mod tests {
    use super::is_picto;

    #[test]
    fn the_electron_app_is_known_by_its_bundle_and_executables() {
        assert!(is_picto("/Applications/Picto.app/Contents/MacOS/Picto"));
        assert!(is_picto(
            "/Applications/Picto.app/Contents/Frameworks/Picto Helper (Renderer).app/Contents/MacOS/Picto Helper (Renderer)"
        ));
        assert!(is_picto(
            "/Users/me/Downloads/Picto 2.app/Contents/MacOS/Picto"
        ));
        assert!(!is_picto(
            "/Users/me/Code/experiments/gpui/target/Picto GPUI.app/Contents/MacOS/picto-gpui-spike"
        ));
        assert!(!is_picto("/usr/bin/pictor"));
        // The process this test runs in is not the Electron app.
        assert!(super::picto_process().is_ok());
    }
}
