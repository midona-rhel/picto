//! Finds out, on macOS, whether the Electron app is running. It reads the
//! executable path of every process (`proc_listallpids`, `proc_pidpath`) and
//! tests each path with [`is_picto`].
use std::ffi::c_void;
use std::path::Path;

/// Returns the executable path of the first process found that belongs to the
/// Electron app, or `None` if there is none. Returns an error if the list of
/// processes could not be read.
pub fn picto_process() -> Result<Option<String>, String> {
    // SAFETY: a null buffer asks for the number of processes.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Err("the list of processes could not be read".into());
    }
    // The buffer has room for 64 more processes, in case some started after
    // the count was read.
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

/// Returns whether the executable at `path` belongs to the Electron app. It
/// does when it is inside a bundle named `Picto.app` (the Electron app's
/// `productName`), as its main process and its helpers are. It also does when
/// its file name is `Picto` or starts with `Picto Helper`, which covers a
/// bundle that the user renamed. This app's own bundle (`Picto GPUI.app`,
/// executable `picto-gpui-spike`) does not.
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

/// Returns a text that identifies the database file at `path`: its device, its
/// inode and the time it was created.
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
            "/home/me/Downloads/Picto 2.app/Contents/MacOS/Picto"
        ));
        assert!(!is_picto(
            "/home/me/Code/experiments/gpui/target/Picto GPUI.app/Contents/MacOS/picto-gpui-spike"
        ));
        assert!(!is_picto("/usr/bin/pictor"));
        // The list of processes can be read. Whether the Electron app is
        // running on this machine is not checked.
        assert!(super::picto_process().is_ok());
    }
}
