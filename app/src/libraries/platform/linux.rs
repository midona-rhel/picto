//! Finds out, on Linux, whether another Picto is running. It reads the
//! executable, the name and the first argument of every process in `/proc`
//! and tests them with [`is_picto`]. This process itself is left out, and so
//! is the AppImage runtime that serves this process's own files.
use std::path::Path;

/// Returns the process id and the executable of the first other process
/// found that is a Picto, or `None` if there is none. Returns an error if
/// `/proc` could not be read.
pub fn picto_process() -> Result<Option<String>, String> {
    let entries = std::fs::read_dir("/proc").map_err(|error| format!("/proc: {error}"))?;
    let own = std::process::id().to_string();
    // A program started from an AppImage has a second process beside it: the
    // AppImage's runtime, which serves the program's files and whose
    // executable is the AppImage file itself. The runtime sets `APPIMAGE` to
    // that file's path. The runtime of this process's own AppImage is part
    // of this launch and is not another Picto. Another Picto started from the
    // same file is still found, by its own program named `picto`.
    let own_image = std::env::var_os("APPIMAGE").map(std::path::PathBuf::from);
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str() else {
            continue;
        };
        if !pid.bytes().all(|byte| byte.is_ascii_digit()) || pid == own {
            continue;
        }
        let folder = entry.path();
        // Each of the three may be unreadable, for another user's process
        // or for a process that has ended. The test uses what can be read.
        let executable = std::fs::read_link(folder.join("exe")).unwrap_or_default();
        if own_image
            .as_deref()
            .is_some_and(|image| image == executable)
        {
            continue;
        }
        let executable = executable.to_string_lossy().into_owned();
        let comm = std::fs::read_to_string(folder.join("comm")).unwrap_or_default();
        let first = std::fs::read(folder.join("cmdline"))
            .ok()
            .and_then(|line| {
                line.split(|byte| *byte == 0)
                    .next()
                    .map(|first| String::from_utf8_lossy(first).into_owned())
            })
            .unwrap_or_default();
        if is_picto(&executable, comm.trim_end(), &first) {
            return Ok(Some(format!("{pid} {executable}")));
        }
    }
    Ok(None)
}

/// Returns whether a process is a Picto: the Electron app, or a packaged
/// release build of this app, which is the Electron app's next version and
/// has its names. It is one when its executable is named `picto`, when the
/// process is named `picto`, or when it was started from a file whose name
/// starts with `Picto` and ends with `.AppImage`. A debug build is named
/// `picto-gpui-spike` (the kernel shortens the process name to
/// `picto-gpui-spik`) and is not a Picto here, so several debug builds can
/// run at once.
pub fn is_picto(executable: &str, comm: &str, first_argument: &str) -> bool {
    let base = |path: &str| {
        Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned()
    };
    let first = base(first_argument);
    base(executable) == "picto"
        || comm == "picto"
        || first == "picto"
                // i18n: a process's name
        || (first.starts_with("Picto") && first.to_lowercase().ends_with(".appimage"))
}

/// Returns a text that identifies the database file at `path`: its device,
/// its inode and the time it was created.
pub fn file_identity(path: &Path) -> std::io::Result<String> {
    super::unix_identity(path)
}

#[cfg(test)]
mod tests {
    use super::is_picto;

    #[test]
    fn a_picto_is_known_by_the_name_of_its_program_or_its_image() {
        assert!(is_picto(
            "/tmp/.mount_PictoX/picto",
            "picto",
            "/tmp/.mount_PictoX/picto"
        ));
        assert!(is_picto("/opt/Picto/picto", "picto", "/opt/Picto/picto"));
        // The packaged app inside its mounted AppImage.
        assert!(is_picto(
            "/tmp/.mount_PictoY/usr/bin/picto",
            "picto",
            "/tmp/.mount_PictoY/usr/bin/picto"
        ));
        assert!(is_picto("", "", "/home/me/Picto-1.0.0.AppImage"));
        assert!(!is_picto(
            "/home/me/gpui/target/debug/picto-gpui-spike",
            "picto-gpui-spik",
            "target/debug/picto-gpui-spike"
        ));
        assert!(!is_picto("/usr/bin/bash", "bash", "bash"));
        assert!(super::picto_process().is_ok());
    }
}
