//! Whether the Electron app runs, on Windows: every process's executable
//! name from a Tool Help snapshot, looked at by [`is_picto`].
use std::ffi::c_void;
use std::path::Path;

const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
const MAX_PATH: usize = 260;

#[repr(C)]
struct ProcessEntry32W {
    dw_size: u32,
    cnt_usage: u32,
    th32_process_id: u32,
    th32_default_heap_id: usize,
    th32_module_id: u32,
    cnt_threads: u32,
    th32_parent_process_id: u32,
    pc_pri_class_base: i32,
    dw_flags: u32,
    sz_exe_file: [u16; MAX_PATH],
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> *mut c_void;
    fn Process32FirstW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
    fn Process32NextW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

/// The executable of the first process of the Electron app found, if any.
pub fn picto_process() -> Result<Option<String>, String> {
    // SAFETY: plain call; the handle is closed below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot.is_null() || snapshot as isize == -1 {
        return Err("the list of processes could not be read".into());
    }
    // SAFETY: an all-zero entry is valid; its size is set as the call asks.
    let mut entry: ProcessEntry32W = unsafe { std::mem::zeroed() };
    entry.dw_size = std::mem::size_of::<ProcessEntry32W>() as u32;
    let own = std::process::id();
    let mut found = None;
    // SAFETY: the snapshot is open and the entry sized.
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while more {
        let length = entry
            .sz_exe_file
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(MAX_PATH);
        let name = String::from_utf16_lossy(&entry.sz_exe_file[..length]);
        if entry.th32_process_id != own && is_picto(&name) {
            found = Some(name);
            break;
        }
        // SAFETY: as above.
        more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    // SAFETY: the handle came from the snapshot call.
    unsafe { CloseHandle(snapshot) };
    Ok(found)
}

/// Whether an executable name is the Electron app's (`Picto.exe`, from its
/// `productName`), whatever its case. This app (`picto-gpui-spike.exe`) is
/// not.
pub fn is_picto(name: &str) -> bool {
    name.eq_ignore_ascii_case("Picto.exe")
}

/// The database file's identity: when it was created, which a copy, a
/// restore or a replacement changes. (Its volume and file index need an
/// unstable interface.)
pub fn file_identity(path: &Path) -> std::io::Result<String> {
    use std::os::windows::fs::MetadataExt as _;
    let meta = std::fs::metadata(path)?;
    Ok(format!("created:{}", meta.creation_time()))
}

#[cfg(test)]
mod tests {
    use super::is_picto;

    #[test]
    fn the_electron_app_is_known_by_its_executable() {
        assert!(is_picto("Picto.exe"));
        assert!(is_picto("PICTO.EXE"));
        assert!(!is_picto("picto-gpui-spike.exe"));
        assert!(!is_picto("Pictogram.exe"));
        assert!(super::picto_process().is_ok());
    }
}
