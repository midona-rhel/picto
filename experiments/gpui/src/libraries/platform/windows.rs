//! Finds out, on Windows, whether another Picto is running. It reads the
//! executable name of every process from a Tool Help snapshot and tests each
//! name with [`is_picto`]. This process itself is left out.
use std::path::Path;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
    TH32CS_SNAPPROCESS,
};

/// Returns the executable name of the first other process found that is a
/// Picto, or `None` if there is none. Returns an error if the list of
/// processes could not be read.
pub fn picto_process() -> Result<Option<String>, String> {
    // SAFETY: the call takes no pointers. The handle it returns is closed
    // below.
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return Err("the list of processes could not be read".into());
    };
    // `Process32FirstW` requires `dwSize` to hold the size of the entry.
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let own = std::process::id();
    let mut found = None;
    // SAFETY: the snapshot handle is open and `entry.dwSize` is set.
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
        // SAFETY: the snapshot handle is open and `entry.dwSize` is set.
        more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    // SAFETY: the handle came from `CreateToolhelp32Snapshot` above.
    let _ = unsafe { CloseHandle(snapshot) };
    Ok(found)
}

/// Returns whether an executable name is a Picto's: `Picto.exe`, in any
/// letter case. The Electron app has that name, and so has a packaged
/// release build of this app, which is the Electron app's next version. A
/// debug build is named `picto-gpui-spike.exe` and is not a Picto here, so
/// several debug builds can run at once.
pub fn is_picto(name: &str) -> bool {
    name.eq_ignore_ascii_case("Picto.exe")
}

/// Returns a text that identifies the database file at `path`: the time the
/// file was created. A copy, a restore or a replacement of the file changes
/// that time. The file's volume and file index would identify it better, but
/// the standard library gives them only through an unstable interface.
pub fn file_identity(path: &Path) -> std::io::Result<String> {
    use std::os::windows::fs::MetadataExt as _;
    let meta = std::fs::metadata(path)?;
    Ok(format!("created:{}", meta.creation_time()))
}

#[cfg(test)]
mod tests {
    use super::is_picto;

    #[test]
    fn a_picto_is_known_by_the_name_of_its_program() {
        assert!(is_picto("Picto.exe"));
        assert!(is_picto("PICTO.EXE"));
        assert!(!is_picto("picto-gpui-spike.exe"));
        assert!(!is_picto("Pictogram.exe"));
        assert!(super::picto_process().is_ok());
    }
}
