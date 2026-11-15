//! Whether the Electron app runs, on Windows: every process's executable
//! name from a Tool Help snapshot, looked at by [`is_picto`].
use std::path::Path;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
    TH32CS_SNAPPROCESS,
};

/// The executable of the first process of the Electron app found, if any.
pub fn picto_process() -> Result<Option<String>, String> {
    // SAFETY: plain call; the handle is closed below.
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return Err("the list of processes could not be read".into());
    };
    // The entry's size is set as the call asks.
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let own = std::process::id();
    let mut found = None;
    // SAFETY: the snapshot is open and the entry sized.
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while more {
        let length = entry
            .szExeFile
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..length]);
        if entry.th32ProcessID != own && is_picto(&name) {
            found = Some(name);
            break;
        }
        // SAFETY: as above.
        more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    // SAFETY: the handle came from the snapshot call.
    let _ = unsafe { CloseHandle(snapshot) };
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
