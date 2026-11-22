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
/// letter case. A packaged release build of this app has that name, and so
/// has an Electron-based Picto (version 0.6.19 or earlier). A debug build
/// is named `picto-gpui-spike.exe` and is not a Picto here, so several
/// debug builds can run at once.
pub fn is_picto(name: &str) -> bool {
    name.eq_ignore_ascii_case("Picto.exe")
}

/// The icon that a library's folder shows in Explorer.
const LIBRARY_ICON: &[u8] = include_bytes!("../../../../build/library.ico");

/// The name of the copy of the icon inside a library's folder. Picto 0.6.19
/// and earlier used the same name.
const LIBRARY_ICON_NAME: &str = ".picto-library.ico";
/// The contents of the `desktop.ini` in a library's folder, which tells
/// Explorer to show the icon above for the folder. Picto 0.6.19 and earlier
/// wrote the same contents.
const DESKTOP_INI: &[u8] = b"[.ShellClassInfo]\r\nIconResource=.picto-library.ico,0\r\n";

/// Gives the folder of the library at `root` Picto's library icon in
/// Explorer. It writes the icon and a `desktop.ini` that names it into the
/// folder, marks both files hidden and system, and marks the folder
/// read-only. Explorer reads a folder's `desktop.ini` only when the folder
/// has the read-only or the system attribute; the attribute does not stop
/// anything from writing into the folder.
pub fn mark_library_folder(root: &Path) -> std::io::Result<()> {
    write_shell_file(&root.join(LIBRARY_ICON_NAME), LIBRARY_ICON)?;
    write_shell_file(&root.join("desktop.ini"), DESKTOP_INI)?;
    attrib(&["+r"], root)
}

/// Writes `contents` to the file at `path` unless the file already holds
/// them, and marks the file hidden and system. Windows refuses to overwrite
/// a hidden file, so the two attributes are removed before a file that
/// exists is written again.
fn write_shell_file(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let previous = std::fs::read(path).ok();
    if previous.as_deref() != Some(contents) {
        if previous.is_some() {
            attrib(&["-h", "-s"], path)?;
        }
        std::fs::write(path, contents)?;
    }
    attrib(&["+h", "+s"], path)
}

/// Runs the system's `attrib` program with `flags` on `path`, as Picto
/// 0.6.19 and earlier did. The program is started without a console window.
fn attrib(flags: &[&str], path: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt as _;
    // `CREATE_NO_WINDOW`: a console program that an app without a console
    // starts would otherwise open a console window for a moment.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = std::process::Command::new("attrib")
        .args(flags)
        .arg(path)
        .creation_flags(CREATE_NO_WINDOW)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "attrib {} {} ended with {status}",
            flags.join(" "),
            path.display()
        )))
    }
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
